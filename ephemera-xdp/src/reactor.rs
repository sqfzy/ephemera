use crate::{
    bpf::{Protocols, transfer_flags, xdp_ip_filter::XdpFilter},
    device::{XdpDevice, XdpDeviceConfig},
};
use libbpf_rs::{MapCore, MapFlags};
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, IpCidr},
};
use std::{
    io,
    net::IpAddr,
    ops::Deref,
    os::fd::AsRawFd,
    sync::{Arc, Mutex, OnceLock},
};
use tracing::debug;

pub use xsk_rs::config::{BindFlags, LibxdpFlags, XdpFlags};

/// Global singleton instance of the XDP Reactor.
pub(crate) static GLOBAL_XDP_REACTOR: OnceLock<XdpReactor> = OnceLock::new();

/// Returns the global shared XDP reactor instance.
/// Polls the reactor in a background thread.
///
/// This drives the event loop, handling underlying XDP socket polling and interface flushing.
/// It alternates between processing packets (busy-looping during bursts) and sleeping
/// via `phy::wait` when idle to save CPU.
pub(crate) fn run_reactor_background(reactor: XdpReactor, wait_timeout: Duration) {
    std::thread::spawn(move || {
        loop {
            let (fd, delay) = {
                let mut reactor_guard = reactor.lock().unwrap();

                // Drain the RX queue and process events.
                // Loop continues as long as state changes (handling bursts).
                while reactor_guard.poll_and_flush().unwrap() == PollResult::SocketStateChanged {}

                let XdpReactorInner {
                    device,
                    iface,
                    sockets,
                    ..
                } = &mut *reactor_guard;

                (
                    device.as_raw_fd(),
                    iface.poll_delay(Instant::now(), sockets),
                )
            };

            // Sleep until I/O events occur or timeout expires.
            smoltcp::phy::wait(fd, delay.or(Some(wait_timeout))).unwrap();
        }
    });
}

/// XDP Reactor - Manages the XDP device, network interface, and socket set.
///
/// The `XdpReactor` is the primary entry point for the XDP network stack.
/// It abstracts away background packet polling, allowing users to focus on
/// socket creation and state management.
#[derive(Clone)]
pub struct XdpReactor(pub(crate) Arc<Mutex<XdpReactorInner>>);

#[bon::bon]
impl XdpReactor {
    #[builder(finish_fn = build)]
    pub fn with_device(
        #[builder(start_fn)] mut device: XdpDevice,

        /// Background thread wating timeout.
        /// Lower values reduce latency but increase CPU usage when idle.
        #[builder(default = Duration::from_millis(10))]
        wait_timeout: Duration,
    ) -> io::Result<Self> {
        let if_name = device.config().if_name.clone();

        let interface = netdev::get_interfaces()
            .into_iter()
            .find(|i| i.name == if_name)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Network interface '{}' not found", if_name),
                )
            })?;

        let xdp_if_name = &interface.name;
        let xdp_if_index = interface.index;

        // 2. Retrieve MAC address
        let xdp_mac = EthernetAddress(
            interface
                .mac_addr
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "No MAC address found for the interface",
                    )
                })?
                .octets(),
        );

        // 3. Load BPF program
        let bpf = XdpFilter::new(
            xdp_if_index as i32,
            transfer_flags(device.config().xdp_flags),
        )
        .map_err(|e| io::Error::other(format!("Failed to load BPF program: {}", e)))?;

        // 4. Initialize smoltcp Interface
        let mut iface = Interface::new(
            smoltcp::iface::Config::new(xdp_mac.into()),
            &mut device,
            Instant::now(),
        );

        for ipv4 in &interface.ipv4 {
            iface.update_ip_addrs(|ip_addrs| {
                ip_addrs
                    .push(IpCidr::new(ipv4.addr().into(), ipv4.prefix_len()))
                    .ok();
            });
        }
        for ipv6 in &interface.ipv6 {
            iface.update_ip_addrs(|ip_addrs| {
                ip_addrs
                    .push(IpCidr::new(ipv6.addr().into(), ipv6.prefix_len()))
                    .ok();
            });
        }
        if iface.ip_addrs().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No IP addresses found for the interface",
            ));
        }

        let gateway = netdev::get_default_gateway()
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        let xdp_gateway_ipv4 = gateway
            .ipv4
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No IPv4 gateway found"))?;
        iface
            .routes_mut()
            .add_default_ipv4_route(xdp_gateway_ipv4)
            .map_err(|e| io::Error::other(format!("Failed to add default route: {}", e)))?;

        // 5. Map queue id to our socket FD in BPF
        let xsk_fd = device.as_raw_fd();
        bpf.skel
            .maps
            .xsks_map
            .update(
                &device.config().queue_id.to_le_bytes(),
                &xsk_fd.to_le_bytes(),
                MapFlags::ANY,
            )
            .map_err(|e| io::Error::other(format!("Failed to update xsks_map: {}", e)))?;

        let inner = XdpReactorInner::new(iface, device, bpf);
        let reactor = XdpReactor(Arc::new(Mutex::new(inner)));

        {
            let guard = reactor.0.lock().unwrap();
            debug!(
                if_name = xdp_if_name,
                queue_id = guard.device.config().queue_id,
                mac = %xdp_mac,
                gateway = %xdp_gateway_ipv4,
                ip_addrs = ?guard.iface.ip_addrs(),
                "XdpReactor initialized"
            );
        }

        run_reactor_background(reactor.clone(), wait_timeout);

        Ok(reactor)
    }

    /// Internal helper to initialize the reactor logic.
    #[builder]
    pub fn new(
        /// Network interface name (e.g., "eth0", "wlan0").
        /// If None, the system's default interface will be used.
        #[builder(into, default = netdev::get_default_interface().unwrap().name)]
        if_name: String,

        /// Background thread wating timeout.
        /// Lower values reduce latency but increase CPU usage when idle.
        #[builder(default = Duration::from_millis(10))]
        wait_timeout: Duration,
    ) -> io::Result<Self> {
        let device = XdpDeviceConfig::builder()
            .if_name(if_name)
            // Load custom xdp program
            .libxdp_flags(LibxdpFlags::XSK_LIBXDP_FLAGS_INHIBIT_PROG_LOAD)
            .build()
            .try_into()
            .map_err(|e| io::Error::other(format!("Failed to create XDP device: {}", e)))?;

        Self::with_device(device).wait_timeout(wait_timeout).build()
    }

    /// Set global XDP reactor instance
    pub fn set_global(reactor: XdpReactor) -> Result<(), XdpReactor> {
        GLOBAL_XDP_REACTOR.set(reactor)
    }

    pub fn global() -> XdpReactor {
        GLOBAL_XDP_REACTOR
            .get_or_init(|| {
                XdpReactor::builder()
                    .build()
                    .expect("Failed to initialize global XdpReactor")
            })
            .clone()
    }

    // ==================== BPF Filter Management ====================

    /// Sets the allowed protocol mask for a specific source IP.
    ///
    /// **Note:** This overwrites any previously set protocols for this IP.
    ///
    /// # Arguments
    ///
    /// * `ip` - The source IP address.
    /// * `proto` - The protocol bitmask (e.g., `PROTO_TCP | PROTO_UDP`).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ephemera_xdp::reactor::XdpReactor;
    /// use ephemera_xdp::bpf::{PROTO_TCP, PROTO_UDP};
    /// use std::net::IpAddr;
    ///
    /// let reactor = XdpReactor::global();
    /// let ip: IpAddr = "192.168.1.100".parse()?;
    ///
    /// // Allow TCP and UDP
    /// reactor.set_allowed_src_ip(ip, PROTO_TCP | PROTO_UDP)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn set_allowed_src_ip(&self, ip: IpAddr, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .set_allowed_src_ip(ip, proto)
            .map_err(io::Error::other)
    }

    /// Sets the allowed protocol mask for a specific destination port.
    ///
    /// **Note:** This overwrites any previously set protocols for this port.
    ///
    /// # Arguments
    ///
    /// * `port` - The destination port number.
    /// * `proto` - The protocol bitmask.
    pub fn set_allowed_dst_port(&self, port: u16, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .set_allowed_dst_port(port, proto)
            .map_err(io::Error::other)
    }

    /// Adds protocols to the allowed mask for a source IP.
    ///
    /// This performs a bitwise OR with existing protocols.
    ///
    /// # Arguments
    ///
    /// * `ip` - The source IP address.
    /// * `proto` - The protocol flag(s) to add.
    pub fn add_allowed_src_ip(&self, ip: IpAddr, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .add_allowed_src_ip(ip, proto)
            .map_err(io::Error::other)
    }

    /// Adds protocols to the allowed mask for a destination port.
    ///
    /// This performs a bitwise OR with existing protocols.
    pub fn add_allowed_dst_port(&self, port: u16, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .add_allowed_dst_port(port, proto)
            .map_err(io::Error::other)
    }

    /// Removes specific protocols from the allowed mask for a source IP.
    ///
    /// # Arguments
    ///
    /// * `ip` - The target IP address.
    /// * `proto` - The protocol flag(s) to remove.
    pub fn remove_allowed_src_ip(&self, ip: IpAddr, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .remove_allowed_src_ip(ip, proto)
            .map_err(io::Error::other)
    }

    /// Removes specific protocols from the allowed mask for a destination port.
    pub fn remove_allowed_dst_port(&self, port: u16, proto: Protocols) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .remove_allowed_dst_port(port, proto)
            .map_err(io::Error::other)
    }

    /// Gets the currently allowed protocol mask for a source IP.
    ///
    /// Returns `PROTO_NONE` if the IP is not in the filter map.
    pub fn get_allowed_src_ip_proto(&self, ip: IpAddr) -> io::Result<Protocols> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .get_allowed_src_ip_proto(ip)
            .map_err(io::Error::other)
    }

    /// Gets the currently allowed protocol mask for a destination port.
    ///
    /// Returns `PROTO_NONE` if the port is not in the filter map.
    pub fn get_allowed_dst_port_proto(&self, port: u16) -> io::Result<Protocols> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .get_allowed_dst_port_proto(port)
            .map_err(io::Error::other)
    }

    /// Completely removes a source IP from the allowed list.
    ///
    /// This deletes the entry from the BPF map entirely.
    pub fn delete_allowed_src_ip(&self, ip: IpAddr) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .delete_allowed_src_ip(ip)
            .map_err(io::Error::other)
    }

    /// Completely removes a destination port from the allowed list.
    ///
    /// This deletes the entry from the BPF map entirely.
    pub fn delete_allowed_dst_port(&self, port: u16) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .delete_allowed_dst_port(port)
            .map_err(io::Error::other)
    }
}

impl Deref for XdpReactor {
    type Target = Arc<Mutex<XdpReactorInner>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Inner struct holding the mutable state of the reactor.
///
/// This is wrapped in an `Arc<Mutex<...>>` by `XdpReactor`.
pub struct XdpReactorInner {
    pub(crate) iface: Interface,
    pub(crate) device: XdpDevice,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) bpf: XdpFilter,
}

impl XdpReactorInner {
    pub(crate) fn new(iface: Interface, device: XdpDevice, bpf: XdpFilter) -> XdpReactorInner {
        XdpReactorInner {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
            bpf,
        }
    }

    /// Polls the interface to advance socket states without flushing.
    pub(crate) fn poll(&mut self) -> PollResult {
        let now = Instant::now();
        self.iface.poll(now, &mut self.device, &mut self.sockets)
    }

    /// Polls the interface and immediately flushes the device to transmit packets.
    pub(crate) fn poll_and_flush(&mut self) -> io::Result<PollResult> {
        let res = self.poll();
        self.device.flush()?;
        Ok(res)
    }

    #[cfg(test)]
    pub(crate) fn poll_timeout(&mut self, timeout: Option<Duration>) -> io::Result<PollResult> {
        let now = Instant::now();

        let delay = self.iface.poll_delay(now, &self.sockets).or(timeout);
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        Ok(res)
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use smoltcp::{
        socket::tcp::{Socket as TcpSocket, State},
        wire::IpEndpoint,
    };
    use std::net::Ipv4Addr;

    #[test]
    fn test_reactor_read_and_write() {
        setup();

        let reactor1 = create_reactor1();
        let reactor2 = create_reactor2();

        let handle1 = add_tcp_socket(&reactor1);
        let handle2 = add_tcp_socket(&reactor2);

        let server_endpoint =
            IpEndpoint::new(INTERFACE_IP1.parse::<Ipv4Addr>().unwrap().into(), 12345);
        let local_endpoint =
            IpEndpoint::new(INTERFACE_IP2.parse::<Ipv4Addr>().unwrap().into(), 12346);

        let mut reactor1 = reactor1.lock().unwrap();
        let mut reactor2 = reactor2.lock().unwrap();

        reactor1
            .sockets
            .get_mut::<TcpSocket>(handle1)
            .listen(server_endpoint)
            .unwrap();
        {
            let XdpReactorInner { iface, sockets, .. } = &mut *reactor2;
            sockets
                .get_mut::<TcpSocket>(handle2)
                .connect(iface.context(), server_endpoint, local_endpoint)
                .unwrap();
        }

        for _ in 0..30 {
            reactor2.poll_and_flush().unwrap();
            reactor1.poll_and_flush().unwrap();

            if reactor1.sockets.get_mut::<TcpSocket>(handle1).state() == State::SynReceived
                && reactor2.sockets.get_mut::<TcpSocket>(handle2).state() == State::SynSent
            {
                break;
            }
        }

        assert_eq!(
            reactor1.sockets.get_mut::<TcpSocket>(handle1).state(),
            State::SynReceived
        );
        assert_eq!(
            reactor2.sockets.get_mut::<TcpSocket>(handle2).state(),
            State::SynSent
        );

        for _ in 0..30 {
            reactor2.poll_and_flush().unwrap();
            reactor1.poll_and_flush().unwrap();

            if reactor1.sockets.get_mut::<TcpSocket>(handle1).state() == State::Established
                && reactor2.sockets.get_mut::<TcpSocket>(handle2).state() == State::Established
            {
                break;
            }
        }

        assert_eq!(
            reactor1.sockets.get_mut::<TcpSocket>(handle1).state(),
            State::Established
        );
        assert_eq!(
            reactor2.sockets.get_mut::<TcpSocket>(handle2).state(),
            State::Established
        );

        let msg = b"Hello from device1";
        reactor1
            .sockets
            .get_mut::<TcpSocket>(handle1)
            .send_slice(msg)
            .unwrap();
        reactor1
            .sockets
            .get_mut::<TcpSocket>(handle1)
            .send_slice(msg)
            .unwrap();
        reactor1
            .sockets
            .get_mut::<TcpSocket>(handle1)
            .send_slice(msg)
            .unwrap();
        assert_eq!(
            reactor1.poll_and_flush().unwrap(),
            PollResult::SocketStateChanged
        );

        assert_eq!(
            reactor2
                .poll_timeout(Some(Duration::from_millis(100)))
                .unwrap(),
            PollResult::SocketStateChanged
        );

        let mut buf = vec![0; msg.len()];
        reactor2
            .sockets
            .get_mut::<TcpSocket>(handle2)
            .recv_slice(&mut buf)
            .unwrap();
        assert_eq!(buf, msg);

        reactor2
            .sockets
            .get_mut::<TcpSocket>(handle2)
            .recv_slice(&mut buf)
            .unwrap();
        assert_eq!(buf, msg);

        reactor2
            .sockets
            .get_mut::<TcpSocket>(handle2)
            .recv_slice(&mut buf)
            .unwrap();
        assert_eq!(buf, msg);
    }
}

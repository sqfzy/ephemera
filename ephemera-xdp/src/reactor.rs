use crate::{bpf::xdp_ip_filter::XdpFilter, device::XdpDevice};
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
    sync::{Arc, LazyLock, Mutex},
};

/// Global singleton instance of the XDP Reactor.
///
/// Initializes the background polling thread lazily upon first access.
pub(crate) static GLOBAL_XDP_REACTOR: LazyLock<XdpReactor> = LazyLock::new(|| {
    let reactor = create_global_reactor();
    let reactor = XdpReactor(Arc::new(Mutex::new(reactor)));
    run_reactor_background(reactor.clone());
    reactor
});

/// Returns the global shared XDP reactor instance.
pub(crate) fn global_reactor() -> XdpReactor {
    GLOBAL_XDP_REACTOR.clone()
}

/// Polls the reactor in a background thread.
///
/// This drives the event loop, handling underlying XDP socket polling and interface flushing.
/// It alternates between processing packets (busy-looping during bursts) and sleeping
/// via `phy::wait` when idle to save CPU.
pub(crate) fn run_reactor_background(reactor: XdpReactor) {
    std::thread::spawn(move || {
        let timeout = Duration::from_millis(10);

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
            smoltcp::phy::wait(fd, delay.or(Some(timeout))).unwrap();
        }
    });
}

/// Auto-detects network configuration and initializes the inner reactor.
fn create_global_reactor() -> XdpReactorInner {
    // Detect default interface and gateway
    let iface = netdev::get_default_interface().expect("No network interfaces found");
    let gateway = netdev::get_default_gateway().expect("No default gateway found");

    let xdp_if_name = &iface.name;
    let xdp_if_index = iface.index;
    let xdp_mac = iface
        .mac_addr
        .expect("No MAC address found for the interface");
    let xdp_ip = iface
        .ip_addrs()
        .into_iter()
        .next()
        .expect("No IP address found for the interface");
    let xdp_gateway_ipv4 = gateway
        .ipv4
        .into_iter()
        .next()
        .expect("No gateway IPv4 address found");

    // Load BPF program and create AF_XDP socket
    let bpf = XdpFilter::new(xdp_if_index as i32).expect("Failed to load BPF program.");
    let mut device = XdpDevice::new(xdp_if_name).expect("Failed to create XDP device.");

    // Initialize smoltcp interface
    let mut iface = Interface::new(
        smoltcp::iface::Config::new(EthernetAddress(xdp_mac.octets()).into()),
        &mut device,
        Instant::now(),
    );

    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(xdp_ip.into(), 24))
            .expect("Address list is full.");
    });

    iface
        .routes_mut()
        .add_default_ipv4_route(xdp_gateway_ipv4)
        .expect("Failed to add default route.");

    // Register the AF_XDP socket FD in the BPF map.
    // Note: Hardcoded to key 0 (RX Queue 0) for this implementation.
    let xsk_fd = device.as_raw_fd();
    bpf.skel
        .maps
        .xsks_map
        .update(&0_i32.to_le_bytes(), &xsk_fd.to_le_bytes(), MapFlags::ANY)
        .expect("Failed to update xsks_map.");

    XdpReactorInner::new(iface, device, bpf)
}

/// XDP Reactor - Manages the XDP device, network interface, and socket set.
///
/// The `XdpReactor` is the primary entry point for the XDP network stack.
/// It abstracts away background packet polling, allowing users to focus on
/// socket creation and state management.
///
/// # Examples
///
/// ```no_run
/// use ephemera_xdp::reactor::XdpReactor;
/// use ephemera_xdp::bpf::PROTO_TCP;
///
/// // 1. Use the global reactor (Recommended)
/// let reactor = XdpReactor::global();
/// reactor.add_allowed_dst_port(8080, PROTO_TCP)?;
///
/// // 2. Create a custom reactor instance
/// let reactor = XdpReactor::new()?;
/// reactor.add_allowed_src_ip("192.168.1.100".parse()?, PROTO_TCP)?;
///
/// // 3. Create bound to a specific interface
/// let reactor = XdpReactor::with_interface("eth0")?;
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Clone)]
pub struct XdpReactor(pub(crate) Arc<Mutex<XdpReactorInner>>);

impl XdpReactor {
    /// Creates a new `XdpReactor` using the default network interface.
    ///
    /// Automatically detects the system's default interface and gateway,
    /// then starts the background polling thread.
    pub fn new() -> io::Result<Self> {
        Self::create_reactor(None)
    }

    /// Creates a new `XdpReactor` bound to a specific network interface.
    ///
    /// # Arguments
    ///
    /// * `interface_name` - The name of the interface (e.g., "eth0", "wlan0").
    pub fn with_interface(interface_name: &str) -> io::Result<Self> {
        Self::create_reactor(Some(interface_name))
    }

    /// Internal helper to initialize the reactor logic.
    fn create_reactor(interface_name: Option<&str>) -> io::Result<Self> {
        // 1. Identify interface
        let iface_info = if let Some(name) = interface_name {
            netdev::get_interfaces()
                .into_iter()
                .find(|i| i.name == name)
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("Network interface '{}' not found", name),
                    )
                })?
        } else {
            netdev::get_default_interface()
                .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?
        };

        let xdp_if_name = &iface_info.name;
        let xdp_if_index = iface_info.index;

        // 2. Retrieve MAC address
        let xdp_mac = EthernetAddress(
            iface_info
                .mac_addr
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "No MAC address found for the interface",
                    )
                })?
                .octets(),
        );

        // 3. Retrieve IP address
        let xdp_ip = iface_info.ip_addrs().into_iter().next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "No IP address found for the interface",
            )
        })?;

        // 4. Retrieve Gateway
        let gateway = netdev::get_default_gateway()
            .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        let xdp_gateway_ipv4 = gateway
            .ipv4
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No IPv4 gateway found"))?;

        // 5. Load BPF program
        let bpf = XdpFilter::new(xdp_if_index as i32)
            .map_err(|e| io::Error::other(format!("Failed to load BPF program: {}", e)))?;

        // 6. Initialize XDP Device (AF_XDP socket)
        let mut device = XdpDevice::new(xdp_if_name)
            .map_err(|e| io::Error::other(format!("Failed to create XDP device: {}", e)))?;

        // 7. Initialize smoltcp Interface
        let mut iface = Interface::new(
            smoltcp::iface::Config::new(xdp_mac.into()),
            &mut device,
            Instant::now(),
        );

        iface.update_ip_addrs(|ip_addrs| {
            ip_addrs
                .push(IpCidr::new(xdp_ip.into(), 24))
                .expect("Failed to add IP address");
        });

        iface
            .routes_mut()
            .add_default_ipv4_route(xdp_gateway_ipv4)
            .map_err(|e| io::Error::other(format!("Failed to add default route: {}", e)))?;

        // 8. Map queue 0 to our socket FD in BPF
        let xsk_fd = device.as_raw_fd();
        bpf.skel
            .maps
            .xsks_map
            .update(&0_i32.to_le_bytes(), &xsk_fd.to_le_bytes(), MapFlags::ANY)
            .map_err(|e| io::Error::other(format!("Failed to update xsks_map: {}", e)))?;

        let inner = XdpReactorInner::new(iface, device, bpf);
        let reactor = XdpReactor(Arc::new(Mutex::new(inner)));

        run_reactor_background(reactor.clone());

        Ok(reactor)
    }

    /// Access the global singleton XDP reactor.
    ///
    /// The global reactor is lazily initialized on first access.
    pub fn global() -> XdpReactor {
        global_reactor()
    }

    // ==================== Configuration Methods (Setters) ====================

    /// Adds an IP address to the network interface.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ephemera_xdp::reactor::XdpReactor;
    /// use smoltcp::wire::{IpCidr, IpAddress};
    ///
    /// let reactor = XdpReactor::global();
    /// let ip = IpCidr::new(IpAddress::v4(192, 168, 1, 100), 24);
    /// reactor.add_ip_address(ip)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn add_ip_address(&self, ip: IpCidr) -> io::Result<()> {
        let mut guard = self.lock().unwrap();
        let mut result = Ok(());

        guard.iface.update_ip_addrs(|ip_addrs| {
            if ip_addrs.push(ip).is_err() {
                result = Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "IP address list is full",
                ));
            }
        });

        result
    }

    /// Removes an IP address from the network interface.
    pub fn remove_ip_address(&self, ip: IpCidr) -> io::Result<()> {
        let mut guard = self.lock().unwrap();
        let mut result = Err(io::Error::new(
            io::ErrorKind::NotFound,
            "IP address not found",
        ));

        guard.iface.update_ip_addrs(|ip_addrs| {
            if let Some(pos) = ip_addrs.iter().position(|&x| x == ip) {
                ip_addrs.remove(pos);
                result = Ok(());
            }
        });

        result
    }

    /// Adds an IPv4 route to the routing table.
    pub fn add_ipv4_route(
        &self,
        cidr: IpCidr,
        gateway: smoltcp::wire::Ipv4Address,
    ) -> io::Result<()> {
        let mut guard = self.lock().unwrap();
        let mut route = smoltcp::iface::Route::new_ipv4_gateway(gateway);
        route.cidr = cidr;

        let mut result = Ok(());

        guard.iface.routes_mut().update(|routes| {
            if routes.push(route).is_err() {
                result = Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "Route table is full",
                ));
            }
        });

        result
    }

    /// Sets the default IPv4 gateway.
    pub fn set_default_gateway(&self, gateway: smoltcp::wire::Ipv4Address) -> io::Result<()> {
        let mut guard = self.lock().unwrap();
        guard
            .iface
            .routes_mut()
            .add_default_ipv4_route(gateway)
            .map_err(|e| io::Error::other(format!("Failed to set default gateway: {}", e)))?;

        Ok(())
    }

    // ==================== Query Methods (Getters) ====================

    /// Returns a list of currently configured IP addresses.
    pub fn ip_addresses(&self) -> Vec<IpCidr> {
        let guard = self.lock().unwrap();
        guard.iface.ip_addrs().to_vec()
    }

    /// Returns the hardware (MAC) address of the network interface.
    pub fn hardware_address(&self) -> smoltcp::wire::HardwareAddress {
        let guard = self.lock().unwrap();
        guard.iface.hardware_addr()
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
    pub fn set_allowed_src_ip(&self, ip: IpAddr, proto: u8) -> io::Result<()> {
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
    pub fn set_allowed_dst_port(&self, port: u16, proto: u8) -> io::Result<()> {
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
    /// reactor.add_allowed_src_ip(ip, PROTO_TCP)?;
    /// reactor.add_allowed_src_ip(ip, PROTO_UDP)?; // Now both TCP and UDP are allowed
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn add_allowed_src_ip(&self, ip: IpAddr, proto: u8) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .add_allowed_src_ip(ip, proto)
            .map_err(io::Error::other)
    }

    /// Adds protocols to the allowed mask for a destination port.
    ///
    /// This performs a bitwise OR with existing protocols.
    pub fn add_allowed_dst_port(&self, port: u16, proto: u8) -> io::Result<()> {
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
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ephemera_xdp::reactor::XdpReactor;
    /// use ephemera_xdp::bpf::{PROTO_TCP, PROTO_UDP, PROTO_ALL};
    /// use std::net::IpAddr;
    ///
    /// let reactor = XdpReactor::global();
    /// let ip: IpAddr = "192.168.1.100".parse()?;
    ///
    /// // Assume TCP and UDP are currently allowed
    /// reactor.remove_allowed_src_ip(ip, PROTO_TCP)?; // Removes TCP, keeps UDP
    /// reactor.remove_allowed_src_ip(ip, PROTO_ALL)?; // Removes all protocols
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn remove_allowed_src_ip(&self, ip: IpAddr, proto: u8) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .remove_allowed_src_ip(ip, proto)
            .map_err(io::Error::other)
    }

    /// Removes specific protocols from the allowed mask for a destination port.
    pub fn remove_allowed_dst_port(&self, port: u16, proto: u8) -> io::Result<()> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .remove_allowed_dst_port(port, proto)
            .map_err(io::Error::other)
    }

    /// Gets the currently allowed protocol mask for a source IP.
    ///
    /// Returns `PROTO_NONE` if the IP is not in the filter map.
    pub fn get_allowed_src_ip_proto(&self, ip: IpAddr) -> io::Result<u8> {
        let guard = self.lock().unwrap();
        guard
            .bpf
            .get_allowed_src_ip_proto(ip)
            .map_err(io::Error::other)
    }

    /// Gets the currently allowed protocol mask for a destination port.
    ///
    /// Returns `PROTO_NONE` if the port is not in the filter map.
    pub fn get_allowed_dst_port_proto(&self, port: u16) -> io::Result<u8> {
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


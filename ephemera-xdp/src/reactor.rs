use crate::{bpf::xdp_ip_filter::XdpFilter, device::XdpDevice};
use libbpf_rs::{MapCore, MapFlags};
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, IpCidr},
};
use std::{
    io,
    os::fd::AsRawFd,
    sync::{LazyLock, Mutex, MutexGuard},
};

pub(crate) static XDP_REACTOR: LazyLock<Mutex<XdpReactor>> = LazyLock::new(|| {
    let reactor = create_global_reactor();

    run_global_reactor_background();

    Mutex::new(reactor)
});

pub(crate) fn global_reactor() -> MutexGuard<'static, XdpReactor> {
    XDP_REACTOR.lock().unwrap()
}

// Reactor poll in background thread, so user should just care the state change of sockets.
pub(crate) fn run_global_reactor_background() {
    std::thread::spawn(move || {
        let timeout = Duration::from_millis(10);

        loop {
            let (fd, delay) = {
                let mut reactor_guard = global_reactor();
                let reactor = &mut *reactor_guard;

                while reactor.poll_and_flush().unwrap() == PollResult::SocketStateChanged {}

                (
                    reactor.device.as_raw_fd(),
                    reactor.iface.poll_delay(Instant::now(), &reactor.sockets),
                )
            };

            smoltcp::phy::wait(fd, delay.or(Some(timeout))).unwrap();
        }
    });
}

fn create_global_reactor() -> XdpReactor {
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

    // setup BPF
    let bpf = XdpFilter::new(xdp_if_index as i32).expect("Failed to load BPF program.");

    // setup device
    let mut device = XdpDevice::new(xdp_if_name).expect("Failed to create XDP device.");

    // setup interface
    let mut iface = Interface::new(
        smoltcp::iface::Config::new(EthernetAddress(xdp_mac.octets()).into()),
        &mut device,
        Instant::now(),
    );
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(xdp_ip.into(), 24))
            .expect("Address is full.");
    });
    iface
        .routes_mut()
        .add_default_ipv4_route(xdp_gateway_ipv4)
        .expect("Failed to add default route.");

    let xsk_fd = device.as_raw_fd();
    bpf.skel
        .maps
        .xsks_map
        .update(
            &0_i32.to_le_bytes(), // 硬编码为0，即队列0
            &xsk_fd.to_le_bytes(),
            MapFlags::ANY,
        )
        .expect("Failed to update xsks_map.");
    // for ip in xdp_allowed_ips {
    //     bpf.add_allowed_ip(ip).unwrap();
    // }

    XdpReactor {
        iface,
        device,
        sockets: SocketSet::new(vec![]),
        bpf,
    }
}

pub(crate) struct XdpReactor {
    pub(crate) iface: Interface,
    pub(crate) device: XdpDevice,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) bpf: XdpFilter,
}

impl XdpReactor {
    /// Poll the interface, try to make all sockets state advance.
    /// Usually, you should call `poll` before the socket recvice something; and call `poll_and_wakeup` after the socket send something.
    pub(crate) fn poll(&mut self) -> PollResult {
        let now = Instant::now();

        self.iface.poll(now, &mut self.device, &mut self.sockets)
    }

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

    // pub(crate) fn poll_timeout_and_flush(
    //     &mut self,
    //     timeout: Option<Duration>,
    // ) -> io::Result<PollResult> {
    //     let now = Instant::now();
    //
    //     let delay = self.iface.poll_delay(now, &self.sockets).or(timeout);
    //     smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;
    //
    //     let res = self.iface.poll(now, &mut self.device, &mut self.sockets);
    //
    //     // PERF:
    //     self.device.flush()?;
    //
    //     Ok(res)
    // }

    // pub(crate) fn flush(&mut self) -> io::Result<usize> {
    //     self.device.flush()
    // }

    #[cfg(test)]
    pub(crate) fn add_tcp_socket(&mut self) -> smoltcp::iface::SocketHandle {
        use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer};

        let socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );
        self.sockets.add(socket)
    }

    // pub(crate) fn waiter(&mut self) -> impl FnOnce() -> io::Result<()> {
    //     let delay = self.iface.poll_delay(Instant::now(), &self.sockets);
    //     let fd = self.device.as_raw_fd();
    //     move || smoltcp::phy::wait(fd, delay)
    // }
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

        let mut reactor1 = create_reactor1();
        let mut reactor2 = create_reactor2();

        let handle1 = reactor1.add_tcp_socket();
        let handle2 = reactor2.add_tcp_socket();

        let server_endpoint =
            IpEndpoint::new(INTERFACE_IP1.parse::<Ipv4Addr>().unwrap().into(), 12345);
        let local_endpoint =
            IpEndpoint::new(INTERFACE_IP2.parse::<Ipv4Addr>().unwrap().into(), 12346);

        reactor1
            .sockets
            .get_mut::<TcpSocket>(handle1)
            .listen(server_endpoint)
            .unwrap();
        reactor2
            .sockets
            .get_mut::<TcpSocket>(handle2)
            .connect(reactor2.iface.context(), server_endpoint, local_endpoint)
            .unwrap();

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

use crate::{bpf::xdp_ip_filter::XdpFilter, device::XdpDevice};
use libbpf_rs::{MapCore, MapFlags};
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, IpCidr},
};
use std::{
    io,
    ops::Deref,
    os::fd::AsRawFd,
    sync::{Arc, LazyLock, Mutex},
};

pub(crate) static GLOBAL_XDP_REACTOR: LazyLock<XdpReactor> = LazyLock::new(|| {
    let reactor = create_global_reactor();
    let reactor = XdpReactor(Arc::new(Mutex::new(reactor)));

    // 启动全局后台线程
    run_reactor_background(reactor.clone());

    reactor
});

pub(crate) fn global_reactor() -> XdpReactor {
    GLOBAL_XDP_REACTOR.clone()
}

// Reactor poll in background thread, so user should just care the state change of sockets.
pub(crate) fn run_reactor_background(reactor: XdpReactor) {
    std::thread::spawn(move || {
        let timeout = Duration::from_millis(10);

        loop {
            let (fd, delay) = {
                let mut reactor_guard = reactor.lock().unwrap();

                // 尽可能推进状态（处理所有待处理的包）
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

            smoltcp::phy::wait(fd, delay.or(Some(timeout))).unwrap();
        }
    });
}

fn create_global_reactor() -> XdpReactorInner {
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

    XdpReactorInner::new(iface, device, bpf)
}

#[derive(Clone)]
pub struct XdpReactor(pub(crate) Arc<Mutex<XdpReactorInner>>);

impl Deref for XdpReactor {
    type Target = Arc<Mutex<XdpReactorInner>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

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

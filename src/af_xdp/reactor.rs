use crate::{af_xdp::device::XdpDevice, bpf::xdp_ip_filter::XdpIpFilter, config::EphemaraConfig};
use libbpf_rs::{MapCore, MapFlags};
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    socket::tcp::{Socket as TcpSocket, SocketBuffer},
    time::{Duration, Instant},
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use std::{
    io,
    os::fd::AsRawFd,
    sync::{LazyLock, Mutex, MutexGuard},
};

pub(crate) static XDP_REACTOR: LazyLock<Mutex<XdpReactor>> = LazyLock::new(|| {
    let EphemaraConfig {
        xdp_if_name,
        xdp_mac,
        xdp_ip,
        xdp_skb_mod,
        xdp_allowed_ips,
        gateway_ip,
        ..
    } = EphemaraConfig::load().unwrap();

    let xdp_mac = xdp_mac
        .parse::<EthernetAddress>()
        .expect("Failed to parse MAC address: {xdp_mac}");

    // setup BPF
    let if_index = nix::net::if_::if_nametoindex(xdp_if_name.as_str()).unwrap() as i32;
    let flags = if xdp_skb_mod {
        libbpf_rs::XdpFlags::SKB_MODE
    } else {
        libbpf_rs::XdpFlags::NONE
    };
    let bpf = XdpIpFilter::new(if_index, flags).unwrap();

    // setup device
    let mut device = XdpDevice::new(&xdp_if_name).unwrap();

    // setup interface
    let mut iface = Interface::new(
        smoltcp::iface::Config::new(xdp_mac.into()),
        &mut device,
        Instant::now(),
    );
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::from(xdp_ip), 24))
            .expect("Address is full.");
    });
    iface
        .routes_mut()
        .add_default_ipv4_route(gateway_ip)
        .unwrap();

    let xsk_fd = device.as_raw_fd();
    bpf.skel
        .maps
        .xsks_map
        .update(
            &0_i32.to_le_bytes(), // 硬编码为0，即队列0
            &xsk_fd.to_le_bytes(),
            MapFlags::ANY,
        )
        .unwrap();
    for ip in xdp_allowed_ips {
        bpf.add_allowed_ip(ip).unwrap();
    }

    Mutex::new(XdpReactor {
        iface,
        device,
        sockets: SocketSet::new(vec![]),
        bpf,
    })
});

pub(crate) fn global_reactor() -> MutexGuard<'static, XdpReactor> {
    XDP_REACTOR.lock().unwrap()
}

pub(crate) struct XdpReactor {
    pub(crate) iface: Interface,
    pub(crate) device: XdpDevice,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) bpf: XdpIpFilter,
}

impl XdpReactor {
    /// 驱动整个网络栈，处理所有事件。
    pub(crate) fn poll3(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let now = Instant::now();

        // let delay = self.iface.poll_delay(now, &self.sockets).or(timeout);
        // smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        self.iface.poll(now, &mut self.device, &mut self.sockets);

        // PERF:
        self.device.flush()?;

        Ok(())
    }

    /// Poll the interface, try to make all sockets state advance.
    /// Usually, you should call `poll` before the socket recvice something; and call `poll_and_wakeup` after the socket send something.
    pub(crate) fn poll(&mut self) -> PollResult {
        let now = Instant::now();

        self.iface.poll(now, &mut self.device, &mut self.sockets)
    }

    pub(crate) fn poll_and_flush(&mut self) -> io::Result<PollResult> {
        let now = Instant::now();

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        self.device.flush()?;

        Ok(res)
    }

    pub(crate) fn poll_timeout(&mut self, timeout: Duration) -> io::Result<PollResult> {
        let now = Instant::now();

        let delay = self.iface.poll_delay(now, &self.sockets).or(Some(timeout));
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        Ok(res)
    }

    pub(crate) fn poll_timeout_and_flush(&mut self, timeout: Duration) -> io::Result<PollResult> {
        let now = Instant::now();

        let delay = self.iface.poll_delay(now, &self.sockets).or(Some(timeout));
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        // PERF:
        self.device.flush()?;

        Ok(res)
    }

    pub(crate) fn add_tcp_socket(&mut self) -> smoltcp::iface::SocketHandle {
        let socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );
        self.sockets.add(socket)
    }
}

#[cfg(test)]
#[serial_test::serial(xdp)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use smoltcp::{
        socket::tcp::{Socket as TcpSocket, State},
        wire::IpEndpoint,
    };
    use std::{net::Ipv4Addr, str::FromStr};

    fn create_reactor1() -> XdpReactor {
        let mac = INTERFACE_MAC1.parse::<EthernetAddress>().unwrap();

        let if_index = nix::net::if_::if_nametoindex(INTERFACE_NAME1).unwrap() as i32;
        let bpf = XdpIpFilter::new(if_index, libbpf_rs::XdpFlags::SKB_MODE).unwrap();

        let mut device = XdpDevice::new(INTERFACE_NAME1).unwrap();
        let mut iface = Interface::new(
            smoltcp::iface::Config::new(mac.into()),
            &mut device,
            Instant::now(),
        );
        iface.update_ip_addrs(|ip_addrs| {
            ip_addrs
                .push(IpCidr::new(IpAddress::from_str(INTERFACE_IP1).unwrap(), 24))
                .unwrap()
        });

        let xsk_fd = device.as_raw_fd();
        bpf.skel
            .maps
            .xsks_map
            .update(
                &0_i32.to_le_bytes(), // 硬编码为0，即队列0
                &xsk_fd.to_le_bytes(),
                MapFlags::ANY,
            )
            .unwrap();
        bpf.add_allowed_ip(INTERFACE_IP1.parse::<Ipv4Addr>().unwrap())
            .unwrap();
        bpf.add_allowed_ip(INTERFACE_IP2.parse::<Ipv4Addr>().unwrap())
            .unwrap();

        XdpReactor {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
            bpf,
        }
    }

    fn create_reactor2() -> XdpReactor {
        let mac = INTERFACE_MAC2.parse::<EthernetAddress>().unwrap();

        let if_index = nix::net::if_::if_nametoindex(INTERFACE_NAME2).unwrap() as i32;
        let bpf = XdpIpFilter::new(if_index, libbpf_rs::XdpFlags::SKB_MODE).unwrap();

        let mut device = XdpDevice::new(INTERFACE_NAME2).unwrap();
        let mut iface = Interface::new(
            smoltcp::iface::Config::new(mac.into()),
            &mut device,
            Instant::now(),
        );
        iface.update_ip_addrs(|ip_addrs| {
            ip_addrs
                .push(IpCidr::new(IpAddress::from_str(INTERFACE_IP2).unwrap(), 24))
                .unwrap()
        });

        let xsk_fd = device.as_raw_fd();
        bpf.skel
            .maps
            .xsks_map
            .update(
                &0_i32.to_le_bytes(), // 硬编码为0，即队列0
                &xsk_fd.to_le_bytes(),
                MapFlags::ANY,
            )
            .unwrap();
        bpf.add_allowed_ip(INTERFACE_IP1.parse::<Ipv4Addr>().unwrap())
            .unwrap();
        bpf.add_allowed_ip(INTERFACE_IP2.parse::<Ipv4Addr>().unwrap())
            .unwrap();

        XdpReactor {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
            bpf,
        }
    }

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
        assert_eq!(
            reactor1.poll_and_flush().unwrap(),
            PollResult::SocketStateChanged
        );

        assert_eq!(
            reactor2.poll_timeout(Duration::from_millis(100)).unwrap(),
            PollResult::SocketStateChanged
        );

        let mut buf = vec![0; msg.len()];
        reactor2
            .sockets
            .get_mut::<TcpSocket>(handle2)
            .recv_slice(&mut buf)
            .unwrap();

        assert_eq!(buf, msg);
    }

    #[test]
    fn test_real_reactor() {
        setup();

        let server_ip = "36.152.44.132";
        let client_ip = "192.168.2.8";

        let mut reactor = global_reactor();

        let handle = reactor.add_tcp_socket();

        let server_endpoint =
            IpEndpoint::new(server_ip.parse::<Ipv4Addr>().unwrap().into(), 80);
        let local_endpoint = IpEndpoint::new(client_ip.parse::<Ipv4Addr>().unwrap().into(), 12346);

        {
            let XdpReactor { iface, sockets, .. } = &mut *reactor;
            sockets
                .get_mut::<TcpSocket>(handle)
                .connect(iface.context(), server_endpoint, local_endpoint)
                .unwrap();
        }

        for _ in 0..30 {
            reactor.poll_and_flush().unwrap();

            if reactor.sockets.get_mut::<TcpSocket>(handle).state() == State::SynSent {
                break;
            }
        }

        assert_eq!(
            reactor.sockets.get_mut::<TcpSocket>(handle).state(),
            State::SynSent
        );

        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            reactor.poll_and_flush().unwrap();

            if reactor.sockets.get_mut::<TcpSocket>(handle).state() == State::Established {
                break;
            }
        }

        assert_eq!(
            reactor.sockets.get_mut::<TcpSocket>(handle).state(),
            State::Established
        );
    }
}

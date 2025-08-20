use crate::{af_xdp::device::XdpDevice, bpf::xdp_ip_filter::XdpIpFilter, config::EphemaraConfig};
use libbpf_rs::{
    MapCore, MapFlags, ObjectBuilder,
    skel::{OpenSkel, SkelBuilder},
};
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};
use std::{
    io,
    mem::MaybeUninit,
    os::fd::AsRawFd,
    process::Command,
    sync::{LazyLock, Mutex, MutexGuard},
};
use xsk_rs::config::{LibxdpFlags, SocketConfig, XdpFlags};

pub(crate) static XDP_REACTOR: LazyLock<Mutex<XdpReactor>> =
    LazyLock::new(|| Mutex::new(XdpReactor::new().expect("Failed to create XdpReactor")));

pub(crate) fn with_reactor<F, R>(f: F) -> R
where
    F: FnOnce(&mut XdpReactor) -> R,
{
    f(&mut XDP_REACTOR.lock().unwrap())
}

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
    pub(crate) fn new() -> eyre::Result<Self> {
        let EphemaraConfig {
            xdp_if_name,
            xdp_mac,
            xdp_ip,
            xdp_skb_mod,
            xdp_allowed_ips,
            gateway_ip,
            ..
        } = EphemaraConfig::load()?;

        let xdp_mac = xdp_mac
            .parse::<EthernetAddress>()
            .map_err(|_| eyre::eyre!("Failed to parse MAC address: {xdp_mac}"))?;

        // setup BPF
        let if_index = nix::net::if_::if_nametoindex(xdp_if_name.as_str())? as i32;
        let flags = if xdp_skb_mod {
            libbpf_rs::XdpFlags::SKB_MODE
        } else {
            libbpf_rs::XdpFlags::NONE
        };
        let bpf = XdpIpFilter::new(if_index, flags)?;

        // setup device
        let mut device = XdpDevice::new(&xdp_if_name)?;

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
        iface.routes_mut().add_default_ipv4_route(gateway_ip)?;

        println!("debug0: fd={}", device.as_raw_fd() as i32);
        let xsk_fd=  device.as_raw_fd();
        bpf.skel.maps.xsks_map.update(
            &0_i32.to_le_bytes(), // 硬编码为0，即队列0
            &xsk_fd.to_le_bytes(),
            MapFlags::ANY,
        )?;
        for ip in xdp_allowed_ips {
            bpf.add_allowed_ip(ip)?;
        }

        Ok(Self {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
            bpf,
        })
    }

    /// 驱动整个网络栈，处理所有事件。
    pub(crate) fn poll3(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let now = Instant::now();

        // let delay = self.iface.poll_delay(now, &self.sockets).or(timeout);
        // smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        self.iface.poll(now, &mut self.device, &mut self.sockets);

        // PERF:
        self.device.wakeup_kernel()?;

        Ok(())
    }

    /// Poll the interface, try to make all sockets state advance.
    /// Usually, you should call `poll` before the socket recvice something; and call `poll_and_wakeup` after the socket send something.
    pub(crate) fn poll(&mut self) -> PollResult {
        let now = Instant::now();

        self.iface.poll(now, &mut self.device, &mut self.sockets)
    }

    pub(crate) fn poll_and_wakeup(&mut self) -> io::Result<PollResult> {
        let now = Instant::now();

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        self.device.wakeup_kernel()?;

        Ok(res)
    }

    pub(crate) fn poll_timeout(&mut self, timeout: Duration) -> io::Result<PollResult> {
        let now = Instant::now();

        let delay = self.iface.poll_delay(now, &self.sockets).or(Some(timeout));
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        let res = self.iface.poll(now, &mut self.device, &mut self.sockets);

        // PERF:
        self.device.wakeup_kernel()?;

        Ok(res)
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use smoltcp::{
        socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
        wire::IpEndpoint,
    };
    use std::net::Ipv4Addr;

    // #[test]
    // fn poll_read_and_write() {
    //     setup();
    //
    //     let device1 = XdpDevice::new(INTERFACE_NAME1).unwrap();
    //     let device2 = XdpDevice::new(INTERFACE_NAME2).unwrap();
    //
    //     let mac1 = INTERFACE_MAC1
    //         .parse::<smoltcp::wire::EthernetAddress>()
    //         .unwrap();
    //     let mac2 = INTERFACE_MAC2
    //         .parse::<smoltcp::wire::EthernetAddress>()
    //         .unwrap();
    //
    //     let mut reactor1 = XdpReactor::new(device1, mac1);
    //     let mut reactor2 = XdpReactor::new(device2, mac2);
    //
    //     // 分别为 reactor1 和 reactor2 设置 IP 地址
    //     reactor1.iface.update_ip_addrs(|ip_addrs| {
    //         ip_addrs
    //             .push(IpCidr::new(
    //                 INTERFACE_IP1.parse::<Ipv4Addr>().unwrap().into(),
    //                 24,
    //             ))
    //             .unwrap();
    //     });
    //     reactor2.iface.update_ip_addrs(|ip_addrs| {
    //         ip_addrs
    //             .push(IpCidr::new(
    //                 INTERFACE_IP2.parse::<Ipv4Addr>().unwrap().into(),
    //                 24,
    //             ))
    //             .unwrap();
    //     });
    //
    //     let handle1 = reactor1.sockets.add(TcpSocket::new(
    //         SocketBuffer::new(vec![0; 1024]),
    //         SocketBuffer::new(vec![0; 1024]),
    //     ));
    //     let handle2 = reactor2.sockets.add(TcpSocket::new(
    //         SocketBuffer::new(vec![0; 1024]),
    //         SocketBuffer::new(vec![0; 1024]),
    //     ));
    //
    //     let server_endpoint =
    //         IpEndpoint::new(INTERFACE_IP1.parse::<Ipv4Addr>().unwrap().into(), 12345);
    //     let local_endpoint =
    //         IpEndpoint::new(INTERFACE_IP2.parse::<Ipv4Addr>().unwrap().into(), 12346);
    //
    //     reactor1
    //         .sockets
    //         .get_mut::<TcpSocket>(handle1)
    //         .listen(server_endpoint)
    //         .unwrap();
    //     reactor2
    //         .sockets
    //         .get_mut::<TcpSocket>(handle2)
    //         .connect(reactor2.iface.context(), server_endpoint, local_endpoint)
    //         .unwrap();
    //
    //     // ARP协议交互，记录对方mac地址
    //     reactor2.poll_and_wakeup().unwrap();
    //     reactor1.poll_and_wakeup().unwrap();
    //
    //     reactor2.poll_and_wakeup().unwrap();
    //     reactor1.poll_and_wakeup().unwrap();
    //
    //     assert_eq!(
    //         reactor1.sockets.get_mut::<TcpSocket>(handle1).state(),
    //         State::SynReceived
    //     );
    //     assert_eq!(
    //         reactor2.sockets.get_mut::<TcpSocket>(handle2).state(),
    //         State::SynSent
    //     );
    //
    //     reactor2.poll_and_wakeup().unwrap();
    //     reactor1.poll_and_wakeup().unwrap();
    //
    //     assert_eq!(
    //         reactor1.sockets.get_mut::<TcpSocket>(handle1).state(),
    //         State::Established
    //     );
    //     assert_eq!(
    //         reactor2.sockets.get_mut::<TcpSocket>(handle2).state(),
    //         State::Established
    //     );
    //
    //     let msg = b"Hello from device1";
    //     reactor1
    //         .sockets
    //         .get_mut::<TcpSocket>(handle1)
    //         .send_slice(msg)
    //         .unwrap();
    //
    //     assert_eq!(
    //         reactor1.poll_and_wakeup().unwrap(),
    //         PollResult::SocketStateChanged
    //     );
    //
    //     let delay = reactor2.iface.poll_delay(Instant::now(), &reactor2.sockets);
    //     smoltcp::phy::wait(reactor2.device.as_raw_fd(), delay).unwrap();
    //     assert_eq!(
    //         reactor2.poll_and_wakeup().unwrap(),
    //         PollResult::SocketStateChanged
    //     );
    //
    //     let mut buf = vec![0; msg.len()];
    //     reactor2
    //         .sockets
    //         .get_mut::<TcpSocket>(handle2)
    //         .recv_slice(&mut buf)
    //         .unwrap();
    //
    //     assert_eq!(buf, msg);
    // }
}

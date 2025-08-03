use smoltcp::{
    iface::{Interface, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};
use std::{
    io,
    os::fd::AsRawFd,
    sync::{LazyLock, Mutex},
};

use crate::{af_xdp::device::XdpDevice, config::EphemaraConfig};

pub static XDP_REACTOR: LazyLock<Mutex<XdpReactor>> = LazyLock::new(|| {
    let config = EphemaraConfig::load().expect("Failed to load config");

    let device = XdpDevice::new(&config.xsk_if_name).expect("Failed to create XDP device");

    let xsk_mac = config
        .xsk_mac
        .parse::<EthernetAddress>()
        .expect("Failed to parse MAC address");

    let mut reactor = XdpReactor::new(device, xsk_mac);

    reactor.iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::from(config.xsk_ip), 24))
            .unwrap();
    });
    reactor
        .iface
        .routes_mut()
        .add_default_ipv4_route(config.gateway_ip)
        .expect("Failed to add default route");

    Mutex::new(reactor)
});

pub fn with_reactor<F, R>(f: F) -> R
where
    F: FnOnce(&mut XdpReactor) -> R,
{
    f(&mut XDP_REACTOR.lock().unwrap())
}

pub struct XdpReactor<const FRAME_COUNT: usize = 1024>
where
    [(); FRAME_COUNT]: Copy,
{
    pub iface: Interface,
    pub device: XdpDevice<FRAME_COUNT>,
    pub sockets: SocketSet<'static>,
}

impl<const FRAME_COUNT: usize> XdpReactor<FRAME_COUNT>
where
    [(); FRAME_COUNT]: Copy,
{
    pub fn new(mut device: XdpDevice<FRAME_COUNT>, mac_addr: impl Into<HardwareAddress>) -> Self {
        let iface = Interface::new(
            smoltcp::iface::Config::new(mac_addr.into()),
            &mut device,
            Instant::now(),
        );

        Self {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
        }
    }

    /// 驱动整个网络栈，处理所有事件。
    pub fn poll(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        let now = Instant::now();

        self.iface.poll(now, &mut self.device, &mut self.sockets);

        // 发送数据需要唤醒内核
        self.device.wakeup_kernel()?;

        let delay = self.iface.poll_delay(now, &self.sockets).or(timeout);
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        Ok(())
    }
}

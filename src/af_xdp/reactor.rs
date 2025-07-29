use eyre::Result;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp::{Socket, SocketBuffer},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use std::{
    cell::{LazyCell, RefCell},
    io,
    net::ToSocketAddrs,
    os::fd::AsRawFd,
};

use crate::{af_xdp::device::XdpDevice, config::AppConfig};

thread_local! {
    pub static XDP_REACTOR: LazyCell<RefCell<XdpReactor>> = LazyCell::new(|| {
        let config = AppConfig::load().expect("Failed to load config");

        let mut device = XdpDevice::new(&config.xsk_if_name).expect("Failed to create XDP device");

        let xsk_mac = config
            .xsk_mac
            .parse::<EthernetAddress>()
            .expect("Failed to parse MAC address");

        let mut iface = Interface::new(
            smoltcp::iface::Config::new(xsk_mac.into()),
            &mut device,
            Instant::now(),
        );
        iface.update_ip_addrs(|ip_addrs| {
            ip_addrs
                .push(IpCidr::new(IpAddress::from(config.xsk_ip), 24))
                .unwrap();
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(config.gateway_ip)
            .expect("Failed to add default route");

        RefCell::new(XdpReactor::new(iface, device))
    });
}

pub fn with_reactor<F, R>(f: F) -> R
where
    F: FnOnce(&mut XdpReactor) -> R,
{
    XDP_REACTOR.with(|reactor| f(&mut reactor.borrow_mut()))
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
    pub fn new(iface: Interface, device: XdpDevice<FRAME_COUNT>) -> Self {
        Self {
            iface,
            device,
            sockets: SocketSet::new(vec![]),
        }
    }

    /// 创建一个新的 TCP 连接并返回其句柄。
    pub fn connect(&mut self, addr: impl ToSocketAddrs, local_port: u16) -> Result<SocketHandle> {
        let tcp_socket = Socket::new(
            SocketBuffer::new(vec![0; 4 * 1024]),
            SocketBuffer::new(vec![0; 4 * 1024]),
        );
        let handle = self.sockets.add(tcp_socket);

        let socket = self.sockets.get_mut::<Socket>(handle);

        let mut addrs = addr.to_socket_addrs()?.peekable();
        while let Some(addr) = addrs.next() {
            match socket.connect(self.iface.context(), addr, local_port) {
                Ok(_) => break,
                Err(_) if addrs.peek().is_some() => continue,
                // 最后的地址也连接失败
                e => e?,
            }
        }

        Ok(handle)
    }

    /// 驱动整个网络栈，处理所有事件。
    pub fn poll(&mut self, timestamp: Instant) -> io::Result<()> {
        self.iface
            .poll(timestamp, &mut self.device, &mut self.sockets);

        // 发送数据需要唤醒内核
        self.device.wakeup_kernel()?;

        let delay = self.iface.poll_delay(timestamp, &self.sockets);
        smoltcp::phy::wait(self.device.as_raw_fd(), delay)?;

        Ok(())
    }

    pub fn socket_mut(&mut self, handle: SocketHandle) -> &mut Socket<'static> {
        self.sockets.get_mut::<Socket>(handle)
    }
}

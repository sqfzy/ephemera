use eyre::Result;
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    socket::tcp::{Socket, SocketBuffer},
    time::Instant,
};
use std::{cell::RefCell, io, net::SocketAddr, os::fd::AsRawFd};

use crate::af_xdp::device::XdpDevice;

thread_local! {
    static REACTOR: RefCell<Option<XdpReactor>> = const { RefCell::new(None) };
}

pub fn with_reactor<F, R>(f: F) -> R
where
    F: FnOnce(&mut XdpReactor) -> R,
{
    REACTOR.with(|reactor_cell| {
        let mut reactor_opt = reactor_cell.borrow_mut();
        let reactor = reactor_opt
            .as_mut()
            .expect("Reactor not initialized for this thread");
        f(reactor)
    })
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
    pub fn new(mut device: XdpDevice<FRAME_COUNT>, config: smoltcp::iface::Config) -> Self {
        let iface = Interface::new(config, &mut device, Instant::now());
        let sockets = SocketSet::new(vec![]);

        Self {
            iface,
            device,
            sockets,
        }
    }

    /// 创建一个新的 TCP 连接并返回其句柄。
    pub fn connect(&mut self, addr: SocketAddr, local_port: u16) -> Result<SocketHandle> {
        let tcp_socket = Socket::new(
            SocketBuffer::new(vec![0; 4 * 1024]),
            SocketBuffer::new(vec![0; 4 * 1024]),
        );
        let handle = self.sockets.add(tcp_socket);

        let socket = self.sockets.get_mut::<Socket>(handle);
        socket.connect(self.iface.context(), addr, local_port)?;

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

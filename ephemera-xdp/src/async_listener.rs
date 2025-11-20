use crate::{
    async_stream::XdpTcpStream,
    bpf,
    reactor::{XdpReactor, global_reactor},
};
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
};
use std::{
    future::poll_fn,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    task::Poll,
};

pub struct XdpTcpListener {
    pub(crate) handle: SocketHandle,
    pub(crate) reactor: XdpReactor,
}

impl XdpTcpListener {
    /// Bind using the global reactor.
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Self::bind_with_reactor(addr, global_reactor())
    }

    /// Bind using a specific reactor.
    pub fn bind_with_reactor(addr: impl ToSocketAddrs, reactor: XdpReactor) -> io::Result<Self> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid address"))?;

        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        socket.listen(addr).map_err(io::Error::other)?;

        let handle = {
            let mut reactor_guard = reactor.lock().unwrap();
            let handle = reactor_guard.sockets.add(socket);

            reactor_guard
                .bpf
                .add_allowed_dst_port(addr.port(), bpf::PROTO_TCP)
                .map_err(io::Error::other)?;

            handle
        };

        Ok(Self { handle, reactor })
    }

    pub async fn accept(&mut self) -> io::Result<(XdpTcpStream, SocketAddr)> {
        let (remote_addr, local_addr) = poll_fn(|cx| {
            let mut reactor = self.reactor.lock().unwrap();
            reactor.poll_and_flush()?;

            let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

            if !matches!(socket.state(), State::Listen | State::SynReceived)
                && let (Some(ra), Some(la)) = (socket.remote_endpoint(), socket.local_endpoint())
            {
                Poll::Ready(Ok::<_, io::Error>((ra, la)))
            } else {
                socket.register_recv_waker(cx.waker());
                Poll::Pending
            }
        })
        .await?;

        // 成功接受了一个连接，原来的 handle 现在是数据流了。
        // 我们需要用一个新的监听器来替换 self。
        let new_listener = Self::bind_with_reactor(
            (IpAddr::from(local_addr.addr), local_addr.port),
            self.reactor.clone(),
        )?;

        let old_listener = std::mem::replace(self, new_listener);

        Ok((
            convert_listner_to_stream(old_listener),
            SocketAddr::from((remote_addr.addr, remote_addr.port)),
        ))
    }
}

impl Drop for XdpTcpListener {
    fn drop(&mut self) {
        let mut reactor = self.reactor.lock().unwrap();
        reactor.sockets.remove(self.handle);
    }
}

fn convert_listner_to_stream(listner: XdpTcpListener) -> XdpTcpStream {
    let s = XdpTcpStream {
        handle: listner.handle,
        reactor: listner.reactor.clone(),
    };

    // Prevent the reactor from removing the handle
    std::mem::forget(listner);

    s
}

use crate::af_xdp::{
    async_stream::XdpTcpStream,
    reactor::{XdpReactor, global_reactor, with_reactor},
};
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
    wire::IpEndpoint,
};
use std::{
    future::poll_fn,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    task::Poll,
};

#[derive(Debug)]
pub struct XdpTcpListener {
    handle: SocketHandle,
}

impl XdpTcpListener {
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid address"))?;

        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        socket.listen(addr).map_err(io::Error::other)?;

        let mut reactor = global_reactor();
        let handle = reactor.sockets.add(socket);

        Ok(Self { handle })
    }

    // pub async fn bind_with_reactor(addr: IpEndpoint, reactor: &mut XdpReactor) -> io::Result<Self> {
    //     let mut socket = TcpSocket::new(
    //         SocketBuffer::new(vec![0; 4096]),
    //         SocketBuffer::new(vec![0; 4096]),
    //     );
    //
    //     socket.listen(addr).map_err(io::Error::other)?;
    //
    //     let handle = reactor.sockets.add(socket);
    //
    //     Ok(Self { handle })
    // }

    pub async fn accept(&mut self) -> io::Result<(XdpTcpStream, SocketAddr)> {
        println!("debug5");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let mut reactor = global_reactor();
            reactor.poll();

            let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);
            match socket.state() {
                State::Listen | State::SynReceived => {
                    println!("debug0");
                    // socket.register_recv_waker(cx.waker());
                    // Poll::Pending;
                }
                // _ => Poll::Ready(()),
                _ => break,
            }
        }
        println!("debug1");

        poll_fn(|cx| {
            let mut reactor = global_reactor();
            reactor.poll();

            let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);
            match socket.state() {
                State::Listen | State::SynReceived => {
                    socket.register_recv_waker(cx.waker());
                    Poll::Pending
                }
                _ => Poll::Ready(()),
            }
        })
        .await;

        let mut reactor = global_reactor();
        let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

        // 获取远程客户端的地址
        let remote_addr = socket
            .remote_endpoint()
            .expect("Connection should be established");

        // 获取我们正在监听的本地地址
        let local_addr = socket
            .local_endpoint()
            .expect("Connection should be established");

        // 成功接受了一个连接，原来的 handle 现在是数据流了。
        // 我们需要用一个新的监听器来替换 self。
        let stream_handle = self.handle;
        let new_listener = Self::bind((IpAddr::from(local_addr.addr), local_addr.port))?;

        // 将 self 的 handle 更新为新监听器的 handle
        self.handle = new_listener.handle;

        println!("debug6");

        Ok((
            XdpTcpStream {
                handle: stream_handle,
            },
            SocketAddr::from((remote_addr.addr, remote_addr.port)),
        ))
    }
}

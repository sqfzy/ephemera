use crate::af_xdp::{
    reactor::{XdpReactor, with_reactor},
    stream::{XdpTcpStream, XdpTcpStreamBorrow},
};
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
    wire::IpEndpoint,
};
use std::{
    io,
    net::{SocketAddr, ToSocketAddrs},
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

        with_reactor(|reactor| Self::bind_with_reactor(addr.into(), reactor))
    }

    pub fn bind_with_reactor(addr: IpEndpoint, reactor: &mut XdpReactor) -> io::Result<Self> {
        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        socket.listen(addr).map_err(io::Error::other)?;

        let handle = reactor.sockets.add(socket);

        Ok(Self { handle })
    }

    pub fn accept(&mut self) -> io::Result<(XdpTcpStream, SocketAddr)> {
        with_reactor(|reactor| {
            self.accept_with_reactor(reactor)
                .map(|(s, a)| (s.into(), a))
        })
    }

    pub fn accept_with_reactor<'r>(
        &mut self,
        reactor: &'r mut XdpReactor,
    ) -> io::Result<(XdpTcpStreamBorrow<'r>, SocketAddr)> {
        println!("debug5");
        while {
            let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle).state();
            socket == State::Listen || socket == State::SynReceived
        } {
            reactor.poll(None)?;
        }

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
        let new_listener = Self::bind_with_reactor(local_addr, reactor)?;

        // 将 self 的 handle 更新为新监听器的 handle
        self.handle = new_listener.handle;

        println!("debug6");

        Ok((
            XdpTcpStreamBorrow {
                handle: stream_handle,
                reactor,
            },
            SocketAddr::from((remote_addr.addr, remote_addr.port)),
        ))
    }

    pub fn into_incoming(self) -> Incoming {
        Incoming { listener: self }
    }
}

#[derive(Debug)]
pub struct Incoming {
    listener: XdpTcpListener,
}

impl Iterator for Incoming {
    type Item = io::Result<XdpTcpStream>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.listener.accept().map(|(s, _)| s))
    }
}

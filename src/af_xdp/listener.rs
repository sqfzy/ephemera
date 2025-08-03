use crate::af_xdp::{
    reactor::{with_reactor, XdpReactor},
    stream::XdpTcpStream,
};
use eyre::Result;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer},
};
use std::{io, net::ToSocketAddrs};

pub struct XdpTcpListener {
    handle: SocketHandle,
}

impl XdpTcpListener {
    pub fn bind(addr: impl ToSocketAddrs) -> Result<Self> {
        with_reactor(|reactor| Self::bind_with_reactor(addr, reactor))
    }

    fn bind_with_reactor(
        addr: impl ToSocketAddrs,
        reactor: &mut XdpReactor,
    ) -> Result<Self> {
        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Invalid address")
        })?;

        socket.listen(addr)?;

        let handle = reactor.sockets.add(socket);

        Ok(Self { handle })
    }

    pub fn accept(&self) -> Result<(XdpTcpStream, XdpTcpListener)> {
        with_reactor(|reactor| self.accept_with_reactor(reactor))
    }

    fn accept_with_reactor(
        &self,
        reactor: &mut XdpReactor,
    ) -> Result<(XdpTcpStream, XdpTcpListener)> {
        loop {
            reactor.poll(None)?;

            let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

            if socket.is_active() && socket.can_recv() {
                let local_endpoint = socket.local_endpoint().unwrap();
                let new_listener = Self::bind_with_reactor(local_endpoint, reactor)?;

                return Ok((
                    XdpTcpStream {
                        handle: self.handle,
                    },
                    new_listener,
                ));
            }
        }
    }

    pub fn incoming(self) -> Incoming {
        Incoming { listener: self }
    }
}

pub struct Incoming {
    listener: XdpTcpListener,
}

impl Iterator for Incoming {
    type Item = Result<XdpTcpStream>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.listener.accept() {
            Ok((stream, new_listener)) => {
                self.listener = new_listener;
                Some(Ok(stream))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

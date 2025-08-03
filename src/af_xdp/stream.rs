use crate::af_xdp::reactor::{XdpReactor, with_reactor};
use eyre::{ContextCompat, Result};
use portpicker::pick_unused_port;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
};
use std::{
    io::{self, Read, Write},
    net::ToSocketAddrs,
};

pub struct XdpTcpStream {
    handle: SocketHandle,
}

impl XdpTcpStream {
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        with_reactor(|reactor| XdpTcpStreamBorrow::connect(addr, reactor).map(Into::into))
    }
}

impl Read for XdpTcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        with_reactor(|reactor| {
            XdpTcpStreamBorrow {
                handle: self.handle,
                reactor,
            }
            .read(buf)
        })
    }
}

impl Write for XdpTcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        with_reactor(|reactor| {
            XdpTcpStreamBorrow {
                handle: self.handle,
                reactor,
            }
            .write(buf)
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        with_reactor(|reactor| {
            XdpTcpStreamBorrow {
                handle: self.handle,
                reactor,
            }
            .flush()
        })
    }
}

pub struct XdpTcpStreamBorrow<'r> {
    handle: SocketHandle,
    reactor: &'r mut XdpReactor,
}

impl<'r> XdpTcpStreamBorrow<'r> {
    pub fn connect(addr: impl ToSocketAddrs, reactor: &'r mut XdpReactor) -> Result<Self> {
        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        let mut addrs = addr.to_socket_addrs()?.peekable();

        while let Some(addr) = addrs.next() {
            let local_port = pick_unused_port().wrap_err("No available port found")?;
            match socket.connect(reactor.iface.context(), addr, local_port) {
                Ok(_) => break,
                Err(_) if addrs.peek().is_some() => continue,
                // 最后的地址也连接失败
                e => e?,
            }
        }

        let handle = reactor.sockets.add(socket);

        while reactor.sockets.get_mut::<TcpSocket>(handle).state() != State::Established {
            reactor.poll(None)?;
        }

        Ok(Self { handle, reactor })
    }
}

impl Read for XdpTcpStreamBorrow<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reactor
            .sockets
            .get_mut::<TcpSocket>(self.handle)
            .recv_slice(buf)
            .map_err(io::Error::other)
    }
}

impl Write for XdpTcpStreamBorrow<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.reactor
            .sockets
            .get_mut::<TcpSocket>(self.handle)
            .send_slice(buf)
            .map_err(io::Error::other)
    }

    fn flush(&mut self) -> io::Result<()> {
        // 唤醒内核，发送数据
        self.reactor.device.wakeup_kernel()
    }
}

impl From<XdpTcpStreamBorrow<'_>> for XdpTcpStream {
    fn from(borrow: XdpTcpStreamBorrow<'_>) -> Self {
        Self {
            handle: borrow.handle,
        }
    }
}

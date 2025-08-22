use crate::af_xdp::reactor::{XdpReactor, with_reactor};
use eyre::{ContextCompat, Result};
use portpicker::pick_unused_port;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
    time::Duration,
    wire::IPV4_HEADER_LEN,
};
use std::{
    io::{self, Read, Write},
    net::ToSocketAddrs,
};

pub struct XdpTcpStream {
    pub(crate) handle: SocketHandle,
}

impl XdpTcpStream {
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        with_reactor(|reactor| XdpTcpStreamBorrow::connect(addr, reactor).map(Into::into))
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        with_reactor(|reactor| {
            XdpTcpStreamBorrow {
                handle: self.handle,
                reactor,
            }
            .shutdown()
        })
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
    pub(crate) handle: SocketHandle,
    pub(crate) reactor: &'r mut XdpReactor,
}

impl<'r> XdpTcpStreamBorrow<'r> {
    pub fn connect(addr: impl ToSocketAddrs, reactor: &'r mut XdpReactor) -> Result<Self> {
        println!("debug3");
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

        debug_assert_eq!(
            reactor.sockets.get_mut::<TcpSocket>(handle).state(),
            State::SynSent,
        );

        while reactor.sockets.get_mut::<TcpSocket>(handle).state() == State::SynSent {
            reactor.poll3(None)?;
        }

        println!("debug4");
        Ok(Self { handle, reactor })
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        println!("debug7");
        let socket = self.reactor.sockets.get_mut::<TcpSocket>(self.handle);

        socket.close();

        while {
            let state = self
                .reactor
                .sockets
                .get_mut::<TcpSocket>(self.handle)
                .state();
            state != State::Closed && state != State::TimeWait
        } {
            self.reactor.poll3(None)?;
        }

        println!("debug8");

        Ok(())
    }
}

impl Read for XdpTcpStreamBorrow<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let socket = self.reactor.sockets.get_mut::<TcpSocket>(self.handle);

            // check can_recv before shutdown, since there may be some data we haven't handled.
            if socket.can_recv() {
                break;
            }

            if socket.state() != State::Established {
                self.shutdown()?;
                return Ok(0);
            }

            self.reactor.poll3(None)?;
        }

        let n = self
            .reactor
            .sockets
            .get_mut::<TcpSocket>(self.handle)
            .recv_slice(buf)
            .map_err(io::Error::other);

        self.reactor.poll3(None)?;

        n
    }
}

impl Write for XdpTcpStreamBorrow<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            let socket = self.reactor.sockets.get_mut::<TcpSocket>(self.handle);

            // check can_send before shutdown, since there may be some data we haven't handled.
            if socket.can_send() {
                break;
            }

            if !socket.is_active() {
                self.shutdown()?;
                return Ok(0);
            }

            self.reactor.poll3(None)?;
        }

        let n = self
            .reactor
            .sockets
            .get_mut::<TcpSocket>(self.handle)
            .send_slice(buf)
            .map_err(io::Error::other);

        self.reactor.poll3(None)?;

        n
    }

    fn flush(&mut self) -> io::Result<()> {
        // 唤醒内核，发送数据
        self.reactor.device.flush()?;
        Ok(())
    }
}

impl From<XdpTcpStreamBorrow<'_>> for XdpTcpStream {
    fn from(borrow: XdpTcpStreamBorrow<'_>) -> Self {
        Self {
            handle: borrow.handle,
        }
    }
}

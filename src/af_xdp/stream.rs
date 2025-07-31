use eyre::Result;
use smoltcp::{iface::SocketHandle, socket::tcp::State, time::Instant};
use std::{
    fmt::Debug,
    io::{self, Read, Write},
    net::ToSocketAddrs,
};

use crate::af_xdp::reactor::with_reactor;

#[derive(Debug, Clone, Copy)]
pub struct XdpStream {
    handle: SocketHandle,
}

impl XdpStream {
    pub fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        with_reactor(|reactor| {
            let handle = reactor.connect(addr)?;

            while reactor.socket_mut(handle).state() != State::Established {
                reactor.poll(Instant::now())?;
            }

            Ok(Self { handle })
        })
    }
}

impl Read for XdpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        with_reactor(|reactor| {
            loop {
                reactor.poll(Instant::now())?;

                let socket = reactor.socket_mut(self.handle);

                if socket.can_recv() {
                    break socket.recv_slice(buf).map_err(io::Error::other);
                }

                if !socket.is_active() || socket.state() == State::CloseWait {
                    return Ok(0); // EOF
                }
            }
        })
    }
}

impl Write for XdpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        with_reactor(|reactor| {
            loop {
                reactor.poll(Instant::now())?;

                let socket = reactor.socket_mut(self.handle);

                if socket.can_send() {
                    break socket.send_slice(buf).map_err(io::Error::other);
                }

                if !socket.is_active() || socket.state() == State::CloseWait {
                    return Ok(0); // EOF
                }
            }
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

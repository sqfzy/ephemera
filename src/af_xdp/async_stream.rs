use crate::af_xdp::reactor::global_reactor;
use portpicker::pick_unused_port;
use smoltcp::{
    iface::SocketHandle,
    phy::{Device, RxToken},
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State as TcpState},
    time::Instant,
    wire::EthernetFrame,
};
use std::{future::poll_fn, io, net::ToSocketAddrs, task::Poll};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct XdpTcpStream {
    pub(crate) handle: SocketHandle,
}

impl XdpTcpStream {
    pub async fn connect(addr: impl ToSocketAddrs) -> io::Result<XdpTcpStream> {
        let handle = {
            let mut socket = TcpSocket::new(
                SocketBuffer::new(vec![0; 4096]),
                SocketBuffer::new(vec![0; 4096]),
            );

            let mut addrs = addr.to_socket_addrs()?.peekable();
            let local_port = pick_unused_port().ok_or_else(|| {
                io::Error::other("Failed to pick an unused port for TCP connection")
            })?;
            let mut reactor = global_reactor();

            while let Some(addr) = addrs.next() {
                match socket.connect(reactor.iface.context(), addr, local_port) {
                    Ok(_) => break,
                    Err(_) if addrs.peek().is_some() => continue,
                    // 最后的地址也连接失败
                    e => e.map_err(|e| {
                        io::Error::other(format!("Failed to connect to {addr}: {e}"))
                    })?,
                }
            }

            debug_assert_eq!(socket.state(), TcpState::SynSent);

            // TODO: 阻塞，when full
            let handle = reactor.sockets.add(socket);
            reactor.poll();
            handle
        };

        // FIX:
        let mut reactor = global_reactor();
        std::thread::sleep(std::time::Duration::from_secs(1));
        reactor.poll_and_flush().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        reactor.poll_and_flush().unwrap();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            // println!("debug3");
            // if let Some((rx_token, _)) = reactor.device.receive(Instant::now()) {
            //     rx_token.consume(|frame_buf| {
            //         let frame = EthernetFrame::new_checked(frame_buf).unwrap();
            //         println!(
            //             "debug0: src={:?}, dst={:?}, ethertype={:?}, payload={:?}",
            //             frame.src_addr(),
            //             frame.dst_addr(),
            //             frame.ethertype(),
            //             frame.payload(),
            //         );
            //     });
            // }

            reactor.poll_and_flush().unwrap();
            let socket = reactor.sockets.get_mut::<TcpSocket>(handle);

            println!("debug0: state={:?}", socket.state());
            match socket.state() {
                TcpState::Established => break,
                TcpState::SynSent => {
                    // socket.register_send_waker(cx.waker());
                    // Poll::Pending
                }
                _ => unreachable!(),
            }
        }
        println!("debug2");
        poll_fn(|cx| {
            let mut reactor = global_reactor();
            let socket = reactor.sockets.get_mut::<TcpSocket>(handle);

            match socket.state() {
                TcpState::Established => Poll::Ready(Ok(())),
                TcpState::SynSent => {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
                _ => Poll::Ready(Err(io::Error::other(format!(
                    "Failed to connect, unexpected TCP state: {:?}",
                    socket.state()
                )))),
            }
        })
        .await?;

        Ok(Self { handle })
    }
}

impl AsyncRead for XdpTcpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let mut reactor = global_reactor();
        reactor.poll();

        let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

        if !socket.may_recv() {
            return Poll::Ready(Ok(()));
        }

        if socket.can_recv() {
            let n = socket
                .recv_slice(buf.initialize_unfilled())
                .map_err(io::Error::other)?;

            buf.advance(n);

            return Poll::Ready(Ok(()));
        }

        socket.register_recv_waker(cx.waker());
        Poll::Pending
    }
}

impl AsyncWrite for XdpTcpStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        let mut reactor = global_reactor();

        let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

        if !socket.may_send() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        if socket.can_send() {
            let n = socket.send_slice(buf).map_err(io::Error::other)?;

            reactor.poll_and_flush()?;
            return Poll::Ready(Ok(n));
        }

        socket.register_send_waker(cx.waker());
        Poll::Pending
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        let mut reactor = global_reactor();
        reactor.poll_and_flush()?;

        let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);
        if socket.send_queue() == 0 {
            return Poll::Ready(Ok(()));
        }
        socket.register_send_waker(cx.waker());
        Poll::Pending
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let mut reactor = global_reactor();
        reactor.poll_and_flush()?;

        let socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);

        if socket.is_open() {
            socket.close();
        }
        if socket.state() == TcpState::Closed {
            return Poll::Ready(Ok(()));
        }

        socket.register_send_waker(cx.waker());
        Poll::Pending
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use super::*;
    use crate::af_xdp::reactor::{XdpReactor, global_reactor};
    use crate::test_utils::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn stream_connect() {
        setup();

        let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

        tokio::spawn(async move {
            listener.accept().await.unwrap();
        });

        // TcpStream::connect("127.0.0.1:12345").await.unwrap();
        // let _ = XdpTcpStream::connect("180.101.49.44:443").await.unwrap();
        // let _ = XdpTcpStream::connect("127.0.0.1:12345").await.unwrap();

        let addr = "127.0.0.1:12345";
        let handle = {
            // let mut socket = TcpSocket::new(
            //     SocketBuffer::new(vec![0; 4096]),
            //     SocketBuffer::new(vec![0; 4096]),
            // );

            let mut reactor = global_reactor();
            let handle = reactor.add_tcp_socket();

            let XdpReactor { iface, sockets, .. } = &mut *reactor;

            let socket = sockets.get_mut::<TcpSocket>(handle);

            let mut addrs = addr.to_socket_addrs().unwrap().peekable();
            let local_port = pick_unused_port()
                .ok_or_else(|| io::Error::other("Failed to pick an unused port for TCP connection"))
                .unwrap();

            while let Some(addr) = addrs.next() {
                match socket.connect(iface.context(), addr, local_port) {
                    Ok(_) => break,
                    Err(_) if addrs.peek().is_some() => continue,
                    // 最后的地址也连接失败
                    e => e
                        .map_err(|e| io::Error::other(format!("Failed to connect to {addr}: {e}")))
                        .unwrap(),
                }
            }

            debug_assert_eq!(socket.state(), TcpState::SynSent);

            // TODO: 阻塞，when full
            reactor.poll();
            handle
        };

        let mut reactor = global_reactor();

        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            // println!("debug3");
            // if let Some((rx_token, _)) = reactor.device.receive(Instant::now()) {
            //     rx_token.consume(|frame_buf| {
            //         let frame = EthernetFrame::new_checked(frame_buf).unwrap();
            //         println!(
            //             "debug0: src={:?}, dst={:?}, ethertype={:?}, payload={:?}",
            //             frame.src_addr(),
            //             frame.dst_addr(),
            //             frame.ethertype(),
            //             frame.payload(),
            //         );
            //     });
            // }

            reactor.poll_and_flush().unwrap();
            let socket = reactor.sockets.get_mut::<TcpSocket>(handle);

            println!("debug0: state={:?}", socket.state());
            match socket.state() {
                TcpState::Established => break,
                TcpState::SynSent => {
                    // socket.register_send_waker(cx.waker());
                    // Poll::Pending
                }
                _ => unreachable!(),
            }
        }
    }
}

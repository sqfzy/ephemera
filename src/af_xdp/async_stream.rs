use crate::af_xdp::reactor::global_reactor;
use portpicker::pick_unused_port;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State as TcpState},
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

            reactor.sockets.add(socket)
        };

        poll_fn(|cx| {
            let mut reactor = global_reactor();
            reactor.poll_and_flush()?;

            let socket = reactor.sockets.get_mut::<TcpSocket>(handle);

            match socket.state() {
                TcpState::SynSent => {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
                _ => Poll::Ready(Ok::<_, io::Error>(())),
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::test_utils::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_connect() {
        setup();

        let reactor1 = Arc::new(Mutex::new(create_reactor1()));
        let reactor2 = create_reactor2();

        run_reactor_background(reactor1.clone());
        replace_global_reactor(reactor2);

        let port = 12345;

        let mut listener =
            bind_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor1.clone()).unwrap();
        let handle = tokio::spawn(async move {
            accept_with_reactor(&mut listener, reactor1.clone())
                .await
                .unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        XdpTcpStream::connect(format!("{INTERFACE_IP1}:{port}"))
            .await
            .unwrap();

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_and_write() {
        setup();

        let reactor1 = Arc::new(Mutex::new(create_reactor1()));
        let reactor2 = create_reactor2();

        run_reactor_background(reactor1.clone());
        replace_global_reactor(reactor2);

        let port = 12345;
        let msg = b"Hello";

        let mut listener =
            bind_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor1.clone()).unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = accept_with_reactor(&mut listener, reactor1.clone())
                .await
                .unwrap();

            let mut buf = vec![0_u8; msg.len()];
            read_with_reactor(&mut stream, &mut buf, reactor1.clone())
                .await
                .unwrap();

            assert_eq!(&buf, &msg);

            write_with_reactor(&mut stream, &buf, reactor1.clone())
                .await
                .unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut stream = XdpTcpStream::connect(format!("{INTERFACE_IP1}:{port}"))
            .await
            .unwrap();

        stream.write_all(b"Hello".as_slice()).await.unwrap();
        stream.flush().await.unwrap();

        let mut buf = vec![0_u8; msg.len()];
        stream.read_exact(&mut buf).await.unwrap();

        assert_eq!(&buf, &msg);

        handle.await.unwrap();
    }
}

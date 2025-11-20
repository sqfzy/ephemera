use crate::{
    bpf,
    reactor::{XdpReactor, global_reactor},
};
use portpicker::pick_unused_port;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State as TcpState},
};
use std::{future::poll_fn, io, net::ToSocketAddrs, task::Poll};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct XdpTcpStream {
    pub(crate) handle: SocketHandle,
    pub(crate) reactor: XdpReactor,
}

impl XdpTcpStream {
    /// Connect using the global reactor.
    pub async fn connect(addr: impl ToSocketAddrs) -> io::Result<XdpTcpStream> {
        Self::connect_with_reactor(addr, global_reactor()).await
    }

    /// Connect using a specific reactor.
    pub async fn connect_with_reactor(
        addr: impl ToSocketAddrs,
        reactor: XdpReactor,
    ) -> io::Result<XdpTcpStream> {
        let handle = {
            let mut socket = TcpSocket::new(
                SocketBuffer::new(vec![0; 65535]),
                SocketBuffer::new(vec![0; 65535]),
            );

            let mut addrs = addr.to_socket_addrs()?.peekable();
            let local_port = pick_unused_port().ok_or_else(|| {
                io::Error::other("Failed to pick an unused port for TCP connection")
            })?;

            let mut reactor_guard = reactor.lock().unwrap();

            while let Some(addr) = addrs.next() {
                reactor_guard
                    .bpf
                    .add_allowed_src_ip(addr.ip(), bpf::PROTO_TCP)
                    .map_err(|e| {
                        io::Error::other(format!("Failed to add {addr} to allowed IPs: {e}"))
                    })?;

                match socket.connect(reactor_guard.iface.context(), addr, local_port) {
                    Ok(_) => break,
                    Err(_) if addrs.peek().is_some() => continue,
                    e => e.map_err(|e| {
                        io::Error::other(format!("Failed to connect to {addr}: {e}"))
                    })?,
                }
            }

            reactor_guard.sockets.add(socket)
        };

        poll_fn(|cx| {
            let mut reactor_guard = reactor.lock().unwrap();
            reactor_guard.poll_and_flush()?;

            let socket = reactor_guard.sockets.get_mut::<TcpSocket>(handle);

            match socket.state() {
                TcpState::Established => Poll::Ready(Ok(())),
                TcpState::SynSent | TcpState::SynReceived => {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
                // 处理连接失败的情况
                state if state == TcpState::Closed || state == TcpState::TimeWait => {
                    Poll::Ready(Err(io::Error::other("Connection closed/failed")))
                }
                _ => {
                    // 其他状态也等待
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
            }
        })
        .await?;

        Ok(Self { handle, reactor })
    }
}

impl Drop for XdpTcpStream {
    fn drop(&mut self) {
        let mut reactor = self.reactor.lock().unwrap();
        reactor.sockets.remove(self.handle);
    }
}

impl AsyncRead for XdpTcpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        let mut reactor = self.reactor.lock().unwrap();
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
        let mut reactor = self.reactor.lock().unwrap();

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
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<()>> {
        let mut reactor = self.reactor.lock().unwrap();

        let mut socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);
        while socket.send_queue() != 0 {
            reactor.poll_and_flush()?;
            socket = reactor.sockets.get_mut::<TcpSocket>(self.handle);
        }

        Poll::Ready(Ok(()))

        // socket.register_send_waker(cx.waker());
        // Poll::Pending
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let mut reactor = self.reactor.lock().unwrap();
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
    use crate::{async_listener::XdpTcpListener, reactor::run_reactor_background, test_utils::*};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn test_connect() {
        setup();

        let reactor1 = create_reactor1();
        let reactor2 = create_reactor2();

        run_reactor_background(reactor1.clone());
        run_reactor_background(reactor2.clone());

        let port = 12345;

        let mut listener =
            XdpTcpListener::bind_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor1.clone())
                .unwrap();
        let handle = tokio::spawn(async move {
            listener.accept().await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        XdpTcpStream::connect_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor2.clone())
            .await
            .unwrap();

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_and_write() {
        setup();

        let reactor1 = create_reactor1();
        let reactor2 = create_reactor2();

        run_reactor_background(reactor1.clone());
        run_reactor_background(reactor2.clone());

        let port = 12345;
        let msg = b"Hello";

        let mut listener =
            XdpTcpListener::bind_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor1.clone())
                .unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            let mut buf = vec![0_u8; msg.len()];
            stream.read_exact(&mut buf).await.unwrap();

            assert_eq!(&buf, &msg);

            stream.write_all(msg).await.unwrap();
            stream.flush().await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut stream =
            XdpTcpStream::connect_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor2.clone())
                .await
                .unwrap();

        stream.write_all(msg).await.unwrap();
        stream.write_all(msg).await.unwrap();
        stream.flush().await.unwrap();

        let mut buf = vec![0_u8; msg.len()];
        stream.read_exact(&mut buf).await.unwrap();

        assert_eq!(&buf, &msg);

        handle.await.unwrap();
    }
}

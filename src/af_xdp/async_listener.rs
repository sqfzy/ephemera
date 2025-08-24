use crate::af_xdp::{async_stream::XdpTcpStream, reactor::global_reactor};
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

#[derive(Debug)]
pub struct XdpTcpListener {
    pub(crate) handle: SocketHandle,
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

    pub async fn accept(&mut self) -> io::Result<(XdpTcpStream, SocketAddr)> {
        let (remote_addr, local_addr) = poll_fn(|cx| {
            let mut reactor = global_reactor();
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
        let stream_handle = self.handle;
        let new_listener = Self::bind((IpAddr::from(local_addr.addr), local_addr.port))?;

        *self = new_listener;

        Ok((
            XdpTcpStream {
                handle: stream_handle,
            },
            SocketAddr::from((remote_addr.addr, remote_addr.port)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::af_xdp::test_utils::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn test_bind() {
        setup();

        let reactor1 = create_reactor1();
        let reactor2 = Arc::new(Mutex::new(create_reactor2()));

        run_reactor_background(reactor2.clone());
        replace_global_reactor(reactor1);

        let port = 12345;

        let mut listener = XdpTcpListener::bind(format!("{INTERFACE_IP1}:{port}")).unwrap();
        let handle = tokio::spawn(async move {
            listener.accept().await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        connect_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor2)
            .await
            .unwrap();

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_read_and_write() {
        setup();

        let reactor1 = create_reactor1();
        let reactor2 = Arc::new(Mutex::new(create_reactor2()));

        run_reactor_background(reactor2.clone());
        replace_global_reactor(reactor1);

        let port = 12345;
        let msg = b"Hello";

        let mut listener = XdpTcpListener::bind(format!("{INTERFACE_IP1}:{port}")).unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            let mut buf = vec![0_u8; msg.len()];
            stream.read_exact(&mut buf).await.unwrap();

            assert_eq!(&buf, &msg);

            stream.write_all(msg).await.unwrap();
            stream.flush().await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut stream = connect_with_reactor(format!("{INTERFACE_IP1}:{port}"), reactor2.clone())
            .await
            .unwrap();

        write_with_reactor(&mut stream, msg.as_slice(), reactor2.clone())
            .await
            .unwrap();

        let mut buf = vec![0_u8; msg.len()];
        read_with_reactor(&mut stream, &mut buf, reactor2.clone())
            .await
            .unwrap();

        assert_eq!(&buf, &msg);

        handle.await.unwrap();
    }
}

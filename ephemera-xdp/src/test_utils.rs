use crate::{
    async_listener::XdpTcpListener,
    async_stream::XdpTcpStream,
    bpf::xdp_ip_filter::XdpFilter,
    device::XdpDevice,
    reactor::{XdpReactor, global_reactor},
};
use libbpf_rs::{MapCore, MapFlags};
use portpicker::pick_unused_port;
use smoltcp::{
    iface::{Interface, PollResult, SocketSet},
    socket::tcp::{Socket as TcpSocket, SocketBuffer, State},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use std::{
    future::poll_fn,
    io,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    os::fd::AsRawFd,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    task::Poll,
};

pub(crate) const INTERFACE_NAME1: &str = "test_iface1";
pub(crate) const INTERFACE_MAC1: &str = "2a:2b:72:fb:e8:cc";
pub(crate) const INTERFACE_IP1: &str = "192.168.2.9";

pub(crate) const INTERFACE_NAME2: &str = "test_iface2";
pub(crate) const INTERFACE_MAC2: &str = "ea:d8:f6:0e:76:01";
pub(crate) const INTERFACE_IP2: &str = "192.168.2.10";

pub fn setup() {
    static START: OnceLock<()> = OnceLock::new();

    START.get_or_init(|| {
        let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::from_str(&level).unwrap())
            .init();
    });
}

pub(crate) fn create_reactor1() -> XdpReactor {
    let mac = INTERFACE_MAC1.parse::<EthernetAddress>().unwrap();

    let if_index = find_index_by_name(INTERFACE_NAME1).unwrap() as i32;
    let bpf = XdpFilter::new(if_index).unwrap();

    let mut device = XdpDevice::new(INTERFACE_NAME1).unwrap();
    let mut iface = Interface::new(
        smoltcp::iface::Config::new(mac.into()),
        &mut device,
        Instant::now(),
    );
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::from_str(INTERFACE_IP1).unwrap(), 24))
            .unwrap()
    });

    let xsk_fd = device.as_raw_fd();
    bpf.skel
        .maps
        .xsks_map
        .update(
            &0_i32.to_le_bytes(), // 硬编码为0，即队列0
            &xsk_fd.to_le_bytes(),
            MapFlags::ANY,
        )
        .unwrap();
    // bpf.add_allowed_src_ip(INTERFACE_IP1.parse::<IpAddr>().unwrap())
    //     .unwrap();
    // bpf.add_allowed_src_ip(INTERFACE_IP2.parse::<IpAddr>().unwrap())
    //     .unwrap();

    XdpReactor {
        iface,
        device,
        sockets: SocketSet::new(vec![]),
        bpf,
    }
}

pub(crate) fn create_reactor2() -> XdpReactor {
    let mac = INTERFACE_MAC2.parse::<EthernetAddress>().unwrap();

    let if_index = find_index_by_name(INTERFACE_NAME2).unwrap() as i32;
    let bpf = XdpFilter::new(if_index).unwrap();

    let mut device = XdpDevice::new(INTERFACE_NAME2).unwrap();
    let mut iface = Interface::new(
        smoltcp::iface::Config::new(mac.into()),
        &mut device,
        Instant::now(),
    );
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::from_str(INTERFACE_IP2).unwrap(), 24))
            .unwrap()
    });

    let xsk_fd = device.as_raw_fd();
    bpf.skel
        .maps
        .xsks_map
        .update(
            &0_i32.to_le_bytes(), // 硬编码为0，即队列0
            &xsk_fd.to_le_bytes(),
            MapFlags::ANY,
        )
        .unwrap();
    // bpf.add_allowed_src_ip(INTERFACE_IP1.parse::<IpAddr>().unwrap())
    //     .unwrap();
    // bpf.add_allowed_src_ip(INTERFACE_IP2.parse::<IpAddr>().unwrap())
    //     .unwrap();

    XdpReactor {
        iface,
        device,
        sockets: SocketSet::new(vec![]),
        bpf,
    }
}

pub(crate) fn find_index_by_name(name: &str) -> Option<u32> {
    let ifaces = netdev::get_interfaces();
    for iface in ifaces {
        if iface.name == name {
            return Some(iface.index);
        }
    }
    None
}

pub(crate) fn bind_with_reactor(
    addr: impl ToSocketAddrs,
    reactor: Arc<Mutex<XdpReactor>>,
) -> io::Result<XdpTcpListener> {
    let addr = addr
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid address"))?;

    let mut socket = TcpSocket::new(
        SocketBuffer::new(vec![0; 4096]),
        SocketBuffer::new(vec![0; 4096]),
    );

    socket.listen(addr).map_err(io::Error::other)?;

    let mut reactor = reactor.lock().unwrap();
    let handle = reactor.sockets.add(socket);

    reactor
        .bpf
        .add_allowed_dst_port(addr.port())
        .map_err(io::Error::other)?;

    Ok(XdpTcpListener { handle })
}

pub(crate) async fn accept_with_reactor(
    listener: &mut XdpTcpListener,
    reactor: Arc<Mutex<XdpReactor>>,
) -> io::Result<(XdpTcpStream, SocketAddr)> {
    let (remote_addr, local_addr) = poll_fn(|cx| {
        let mut reactor = reactor.lock().unwrap();
        reactor.poll_and_flush()?;

        let socket = reactor.sockets.get_mut::<TcpSocket>(listener.handle);

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
    // 我们需要用一个新的监听器来替换 。
    let stream_handle = listener.handle;
    let new_listener =
        bind_with_reactor((IpAddr::from(local_addr.addr), local_addr.port), reactor)?;

    *listener = new_listener;

    Ok((
        XdpTcpStream {
            handle: stream_handle,
        },
        SocketAddr::from((remote_addr.addr, remote_addr.port)),
    ))
}

pub(crate) async fn connect_with_reactor(
    addr: impl ToSocketAddrs,
    reactor: Arc<Mutex<XdpReactor>>,
) -> io::Result<XdpTcpStream> {
    let handle = {
        let mut socket = TcpSocket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );

        let mut addrs = addr.to_socket_addrs()?.peekable();
        let local_port = pick_unused_port()
            .ok_or_else(|| io::Error::other("Failed to pick an unused port for TCP connection"))?;
        let mut reactor = reactor.lock().unwrap();

        while let Some(addr) = addrs.next() {
            reactor.bpf.add_allowed_src_ip(addr.ip()).map_err(|e| {
                io::Error::other(format!("Failed to add {addr} to allowed IPs: {e}"))
            })?;

            match socket.connect(reactor.iface.context(), addr, local_port) {
                Ok(_) => break,
                Err(_) if addrs.peek().is_some() => continue,
                // 最后的地址也连接失败
                e => {
                    e.map_err(|e| io::Error::other(format!("Failed to connect to {addr}: {e}")))?
                }
            }
        }

        debug_assert_eq!(socket.state(), State::SynSent);

        let handle = reactor.sockets.add(socket);
        reactor.poll_and_flush()?;
        handle
    };

    poll_fn(|cx| {
        let mut reactor = reactor.lock().unwrap();

        let socket = reactor.sockets.get_mut::<TcpSocket>(handle);

        match socket.state() {
            State::SynSent => {
                socket.register_send_waker(cx.waker());
                Poll::Pending
            }
            _ => Poll::Ready(()),
        }
    })
    .await;

    Ok(XdpTcpStream { handle })
}

pub(crate) async fn read_with_reactor(
    stream: &mut XdpTcpStream,
    buf: &mut [u8],
    reactor: Arc<Mutex<XdpReactor>>,
) -> io::Result<usize> {
    poll_fn(|cx| {
        let mut reactor = reactor.lock().unwrap();
        reactor.poll();

        let socket = reactor.sockets.get_mut::<TcpSocket>(stream.handle);

        if !socket.may_recv() {
            return Poll::Ready(Ok(0));
        }

        if socket.can_recv() {
            let n = socket.recv_slice(buf).map_err(io::Error::other)?;
            return Poll::Ready(Ok(n));
        }

        socket.register_recv_waker(cx.waker());
        Poll::Pending
    })
    .await
}

pub(crate) async fn write_with_reactor(
    stream: &mut XdpTcpStream,
    buf: &[u8],
    reactor: Arc<Mutex<XdpReactor>>,
) -> io::Result<usize> {
    poll_fn(|cx| {
        let mut reactor = reactor.lock().unwrap();

        let socket = reactor.sockets.get_mut::<TcpSocket>(stream.handle);

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
    })
    .await
}

pub(crate) fn replace_global_reactor(new_reactor: XdpReactor) {
    let mut reactor = global_reactor();
    *reactor = new_reactor;
}
pub(crate) fn run_reactor_background(reactor1: Arc<Mutex<XdpReactor>>) {
    std::thread::spawn(move || {
        loop {
            let (fd, delay) = {
                let mut reactor1 = reactor1.lock().unwrap();

                // while reactor1.poll_and_flush().unwrap() == PollResult::SocketStateChanged {}

                let XdpReactor {
                    iface,
                    device,
                    sockets,
                    ..
                } = &mut *reactor1;

                (
                    device.as_raw_fd(),
                    iface.poll_delay(Instant::now(), sockets),
                )
            };

            smoltcp::phy::wait(fd, delay).unwrap();

            let mut reactor1 = reactor1.lock().unwrap();

            while reactor1.poll_and_flush().unwrap() == PollResult::SocketStateChanged {}
        }
    });
}

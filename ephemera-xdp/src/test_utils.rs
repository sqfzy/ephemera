use crate::{
    bpf::xdp_ip_filter::XdpFilter,
    device::XdpDevice,
    reactor::{XdpReactor, XdpReactorInner},
};
use libbpf_rs::{MapCore, MapFlags};
use smoltcp::{
    iface::{Interface, SocketHandle},
    socket::tcp::{Socket, SocketBuffer},
    time::Instant,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use std::{
    os::fd::AsRawFd,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
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

    Arc::new(Mutex::new(XdpReactorInner::new(iface, device, bpf)))
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

    Arc::new(Mutex::new(XdpReactorInner::new(iface, device, bpf)))
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

pub(crate) fn add_tcp_socket(reactor: &XdpReactor) -> SocketHandle {
    let mut reactor = reactor.lock().unwrap();
    reactor.sockets.add(Socket::new(
        SocketBuffer::new(vec![0; 4096]),
        SocketBuffer::new(vec![0; 4096]),
    ))
}

// 协议位掩码
pub const PROTO_TCP: u8 = 1 << 0;
pub const PROTO_UDP: u8 = 1 << 1;
pub const PROTO_ICMP: u8 = 1 << 2;
pub const PROTO_ICMPV6: u8 = 1 << 3;
pub const PROTO_ALL: u8 = 0xFF;

// 日志级别
pub const LOG_LEVEL_DEBUG: u8 = 0;
pub const LOG_LEVEL_INFO: u8 = 1;
pub const LOG_LEVEL_WARN: u8 = 2;
pub const LOG_LEVEL_ERROR: u8 = 3;

pub(crate) mod xdp_ip_filter {
    use super::*;
    use libbpf_rs::XdpFlags;
    use libbpf_rs::{
        MapCore, PerfBufferBuilder,
        skel::{OpenSkel, SkelBuilder},
    };
    use std::mem::MaybeUninit;
    use std::net::IpAddr;
    use std::os::fd::AsFd;
    use std::time::Duration;
    use tracing::{debug, error, info, warn};

    include!(concat!(env!("OUT_DIR"), "/xdp_ip_filter.skel.rs"));

    // 事件类型
    const EVENT_PASS: u8 = 1;
    const EVENT_DROP: u8 = 2;
    const EVENT_REDIRECT: u8 = 3;
    const EVENT_PROTO_MISMATCH: u8 = 4;
    const EVENT_INVALID_PACKET: u8 = 5;

    #[repr(C)]
    #[derive(Debug)]
    struct LogEvent {
        timestamp: u64,
        src_ip: [u32; 4],
        dst_ip: [u32; 4],
        src_port: u16,
        dst_port: u16,
        protocol: u8,
        ip_version: u8,
        event_type: u8,
        log_level: u8,
        message: [u8; 64],
    }

    #[repr(C)]
    struct PortRule {
        allowed_protocols: u8,
        reserved: [u8; 3],
    }

    pub(crate) struct XdpFilter {
        pub(crate) xdp_if_index: i32,
        pub(crate) skel: XdpFilterSkel<'static>,
    }

    impl XdpFilter {
        pub(crate) fn new(if_index: i32) -> Result<Self, libbpf_rs::Error> {
            let skel_builder = XdpFilterSkelBuilder::default();

            let open_object = Box::leak(Box::new(MaybeUninit::uninit()));
            let open_skel = skel_builder.open(open_object)?;

            let skel: XdpFilterSkel<'static> = open_skel.load()?;

            let xdp_attacher = libbpf_rs::Xdp::new(skel.progs.xdp_filter_prog.as_fd());
            if xdp_attacher.attach(if_index, XdpFlags::DRV_MODE).is_err() {
                // 不支持原生XDP模式，回退到SKB模式
                xdp_attacher.attach(if_index, XdpFlags::SKB_MODE)?;
            }

            Ok(Self {
                xdp_if_index: if_index,
                skel,
            })
        }

        /// 添加允许的源 IP 地址（带协议过滤）
        ///
        /// # Arguments
        /// * `addr` - IP 地址
        /// * `protocols` - 允许的协议位掩码 (PROTO_TCP | PROTO_UDP | ...)
        pub(crate) fn add_allowed_src_ip(
            &self,
            addr: IpAddr,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            match addr {
                IpAddr::V4(addr) => {
                    let key: [u8; 4] = addr.octets();
                    let value: [u8; 1] = [protocols];

                    self.skel.maps.allowed_src_ips_map_v4.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY, // 使用 ANY 允许更新
                    )?;

                    debug!("Added IPv4 {addr} with protocol mask {protocols:#04x}");
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    let value: [u8; 1] = [protocols];

                    self.skel.maps.allowed_src_ips_map_v6.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;

                    debug!("Added IPv6 {addr} with protocol mask {protocols:#04x}");
                }
            }

            Ok(())
        }

        /// 添加允许的目标端口（带协议过滤）
        ///
        /// # Arguments
        /// * `port` - 端口号
        /// * `protocols` - 允许的协议位掩码 (PROTO_TCP | PROTO_UDP | ...)
        pub(crate) fn add_allowed_dst_port(
            &self,
            port: u16,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 2] = port.to_be_bytes();
            let value = PortRule {
                allowed_protocols: protocols,
                reserved: [0; 3],
            };
            let value_bytes = unsafe {
                std::slice::from_raw_parts(
                    &value as *const PortRule as *const u8,
                    std::mem::size_of::<PortRule>(),
                )
            };

            self.skel.maps.allowed_dst_ports_map.update(
                &key,
                value_bytes,
                libbpf_rs::MapFlags::ANY,
            )?;

            debug!("Added port {port} with protocol mask {protocols:#04x}");

            Ok(())
        }

        /// 设置日志级别
        pub(crate) fn set_log_level(&self, level: u8) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 4] = 0u32.to_ne_bytes();
            let value: [u8; 1] = [level];

            self.skel
                .maps
                .log_level_map
                .update(&key, &value, libbpf_rs::MapFlags::ANY)?;

            debug!("Set log level to {level}");

            Ok(())
        }

        /// 启动日志监听（阻塞式）
        ///
        /// 这个方法会阻塞当前线程，持续接收并打印日志事件
        pub(crate) fn listen_logs(&self) -> Result<(), libbpf_rs::Error> {
            let perf = PerfBufferBuilder::new(&self.skel.maps.log_events)
                .sample_cb(Self::handle_event)
                .lost_cb(Self::handle_lost)
                .build()?;

            info!("Started listening for log events");

            loop {
                perf.poll(Duration::from_millis(100))?;
            }
        }

        fn handle_event(_cpu: i32, data: &[u8]) {
            if data.len() < std::mem::size_of::<LogEvent>() {
                warn!("Received truncated event");
                return;
            }

            let event = unsafe { &*(data.as_ptr() as *const LogEvent) };

            let src_ip = if event.ip_version == 4 {
                Self::format_ipv4(event.src_ip[0])
            } else {
                Self::format_ipv6(&event.src_ip)
            };

            let dst_ip = if event.ip_version == 4 {
                Self::format_ipv4(event.dst_ip[0])
            } else {
                Self::format_ipv6(&event.dst_ip)
            };

            let message = String::from_utf8_lossy(&event.message)
                .trim_end_matches('\0')
                .to_string();

            let log_msg = format!(
                "[{}] IPv{} {} {}:{} -> {}:{} | {}",
                Self::event_type_str(event.event_type),
                event.ip_version,
                Self::proto_str(event.protocol),
                src_ip,
                event.src_port,
                dst_ip,
                event.dst_port,
                message
            );

            match event.log_level {
                LOG_LEVEL_DEBUG => debug!("{}", log_msg),
                LOG_LEVEL_INFO => info!("{}", log_msg),
                LOG_LEVEL_WARN => warn!("{}", log_msg),
                LOG_LEVEL_ERROR => error!("{}", log_msg),
                _ => info!("{}", log_msg),
            }
        }

        fn handle_lost(cpu: i32, count: u64) {
            warn!("Lost {count} events on CPU {cpu}");
        }

        fn format_ipv4(ip: u32) -> String {
            let bytes = ip.to_ne_bytes();
            format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])
        }

        fn format_ipv6(ip: &[u32; 4]) -> String {
            use std::net::Ipv6Addr;
            let mut bytes = [0u8; 16];
            for (i, &word) in ip.iter().enumerate() {
                let word_bytes = word.to_be_bytes();
                bytes[i * 4..(i + 1) * 4].copy_from_slice(&word_bytes);
            }
            Ipv6Addr::from(bytes).to_string()
        }

        fn proto_str(proto: u8) -> &'static str {
            match proto {
                6 => "TCP",
                17 => "UDP",
                1 => "ICMP",
                58 => "ICMPv6",
                44 => "Fragment",
                _ => "Unknown",
            }
        }

        fn event_type_str(event_type: u8) -> &'static str {
            match event_type {
                EVENT_PASS => "PASS",
                EVENT_DROP => "DROP",
                EVENT_REDIRECT => "REDIRECT",
                EVENT_PROTO_MISMATCH => "PROTO_MISMATCH",
                EVENT_INVALID_PACKET => "INVALID_PACKET",
                _ => "UNKNOWN",
            }
        }
    }

    impl Drop for XdpFilter {
        fn drop(&mut self) {
            let xdp_attacher = libbpf_rs::Xdp::new(self.skel.progs.xdp_filter_prog.as_fd());
            xdp_attacher
                .detach(self.xdp_if_index, libbpf_rs::XdpFlags::NONE)
                .ok();

            debug!(
                "XDP program detached from interface index {}",
                self.xdp_if_index
            );
        }
    }
}


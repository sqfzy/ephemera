pub(crate) mod xdp_ip_filter {
    use libbpf_rs::XdpFlags;
    use libbpf_rs::{
        MapCore,
        skel::{OpenSkel, SkelBuilder},
    };
    use std::mem::MaybeUninit;
    use std::net::IpAddr;
    use std::os::fd::AsFd;
    use tracing::debug;

    include!(concat!(env!("OUT_DIR"), "/xdp_ip_filter.skel.rs"));

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

        pub(crate) fn add_allowed_src_ip(&self, addr: IpAddr) -> Result<(), libbpf_rs::Error> {
            match addr {
                IpAddr::V4(addr) => {
                    let key: [u8; 4] = addr.octets();
                    let value: [u8; 1] = [1];

                    self.skel.maps.allowed_src_ips_map_v4.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    let value: [u8; 1] = [1];

                    self.skel.maps.allowed_src_ips_map_v6.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;
                }
            }

            debug!("Added {addr:?} to allowed source IPs.");

            Ok(())
        }

        pub(crate) fn add_allowed_dst_port(&self, port: u16) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 2] = port.to_be_bytes();
            let value: [u8; 1] = [1];

            self.skel
                .maps
                .allowed_dst_ports_map
                .update(&key, &value, libbpf_rs::MapFlags::ANY)?;

            debug!("Added {port} to allowed destination ports.");

            Ok(())
        }
    }

    impl Drop for XdpFilter {
        fn drop(&mut self) {
            let xdp_attacher = libbpf_rs::Xdp::new(self.skel.progs.xdp_filter_prog.as_fd());
            xdp_attacher
                .detach(self.xdp_if_index, libbpf_rs::XdpFlags::NONE)
                .ok();
        }
    }
}

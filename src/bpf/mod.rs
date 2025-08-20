pub(crate) mod xdp_ip_filter {
    use libbpf_rs::{Link, MapFlags, XdpFlags};
    use libbpf_rs::{
        MapCore, ObjectBuilder,
        skel::{OpenSkel, SkelBuilder},
    };
    use std::os::fd::AsFd;
    use std::{mem::MaybeUninit, net::Ipv4Addr};
    use tracing::debug;

    include!(concat!(env!("OUT_DIR"), "/xdp_ip_filter.skel.rs"));

    pub(crate) struct XdpIpFilter {
        pub(crate) xdp_if_index: i32,
        pub(crate) skel: XdpIpFilterSkel<'static>,
    }

    impl XdpIpFilter {
        pub(crate) fn new(if_index: i32, flags: XdpFlags) -> Result<Self, libbpf_rs::Error> {
            let skel_builder = XdpIpFilterSkelBuilder::default();

            let open_object = Box::leak(Box::new(MaybeUninit::uninit()));
            let open_skel = skel_builder.open(open_object)?;

            let skel: XdpIpFilterSkel<'static> = open_skel.load()?;
            // let link = skel.progs.xdp_ip_filter_func.attach()?;

            let xdp_attacher = libbpf_rs::Xdp::new(skel.progs.xdp_ip_filter_func.as_fd());
            xdp_attacher.attach(if_index, flags)?;

            Ok(Self {
                xdp_if_index: if_index,
                skel,
            })
        }

        pub(crate) fn add_allowed_ip(&self, ip_addr: Ipv4Addr) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 4] = u32::from(ip_addr).to_be_bytes();
            let value: [u8; 1] = [1];

            self.skel
                .maps
                .allowed_ips_map
                .update(&key, &value, libbpf_rs::MapFlags::ANY)?;

            debug!("Added {ip_addr:?} to whitelist.");

            Ok(())
        }
    }

    impl Drop for XdpIpFilter {
        fn drop(&mut self) {
            println!("debug6: deatch");
            let xdp_attacher = libbpf_rs::Xdp::new(self.skel.progs.xdp_ip_filter_func.as_fd());
            xdp_attacher
                .detach(self.xdp_if_index, libbpf_rs::XdpFlags::NONE)
                .ok();
        }
    }
}

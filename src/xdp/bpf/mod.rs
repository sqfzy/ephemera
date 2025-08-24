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

            let xdp_attacher = libbpf_rs::Xdp::new(skel.progs.xdp_ip_filter_func.as_fd());
            xdp_attacher.attach(if_index, flags)?;

            Ok(Self {
                xdp_if_index: if_index,
                skel,
            })
        }

        pub(crate) fn add_allowed_ip(&self, addr: IpAddr) -> Result<(), libbpf_rs::Error> {
            match addr {
                IpAddr::V4(addr) => {
                    let key: [u8; 4] = addr.octets();
                    let value: [u8; 1] = [1];

                    self.skel.maps.allowed_ips_map_v4.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    let value: [u8; 1] = [1];

                    self.skel.maps.allowed_ips_map_v6.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;
                }
            }

            debug!("Added {addr:?} to whitelist.");

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

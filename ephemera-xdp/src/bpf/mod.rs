/// Protocol bitmasks used for filtering rules.
pub const PROTO_TCP: u8 = 1 << 0;
pub const PROTO_UDP: u8 = 1 << 1;
pub const PROTO_ICMP: u8 = 1 << 2;
pub const PROTO_ICMPV6: u8 = 1 << 3;
/// Mask allowing all supported protocols.
pub const PROTO_ALL: u8 = 0xFF;
/// Mask denying all protocols.
pub const PROTO_NONE: u8 = 0x00;

pub(crate) mod xdp_ip_filter {
    use super::*;
    use libbpf_rs::XdpFlags;
    use libbpf_rs::{
        MapCore,
        skel::{OpenSkel, SkelBuilder},
    };
    use std::mem::MaybeUninit;
    use std::net::IpAddr;
    use std::os::fd::AsFd;
    use tracing::{debug, warn};

    // Include the generated skeleton code
    include!(concat!(env!("OUT_DIR"), "/xdp_ip_filter.skel.rs"));

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
        /// Loads the BPF program and attaches it to the specified network interface.
        ///
        /// Attempts to attach in native driver mode (DRV_MODE) first for performance.
        /// Falls back to generic SKB mode (SKB_MODE) if the driver doesn't support XDP.
        pub(crate) fn new(if_index: i32) -> Result<Self, libbpf_rs::Error> {
            let skel_builder = XdpFilterSkelBuilder::default();

            let open_object = Box::leak(Box::new(MaybeUninit::uninit()));
            let open_skel = skel_builder.open(open_object)?;

            let skel: XdpFilterSkel<'static> = open_skel.load()?;

            let xdp_attacher = libbpf_rs::Xdp::new(skel.progs.xdp_filter_prog.as_fd());

            // Try attaching in Driver mode first, fallback to SKB mode
            if xdp_attacher.attach(if_index, XdpFlags::DRV_MODE).is_err() {
                xdp_attacher.attach(if_index, XdpFlags::SKB_MODE)?;
            }

            Ok(Self {
                xdp_if_index: if_index,
                skel,
            })
        }

        /// Sets the allowed protocol mask for a specific source IP address.
        ///
        /// This overwrites any existing rules for this IP.
        ///
        /// # Arguments
        ///
        /// * `addr` - The source IP address to filter.
        /// * `protocols` - Bitmask of allowed protocols (e.g., `PROTO_TCP | PROTO_UDP`).
        ///   Use `PROTO_NONE` to block all traffic from this IP.
        pub(crate) fn set_allowed_src_ip(
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
                        libbpf_rs::MapFlags::ANY,
                    )?;

                    debug!("Set IPv4 {addr} with protocol mask {protocols:#04x}");
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    let value: [u8; 1] = [protocols];

                    self.skel.maps.allowed_src_ips_map_v6.update(
                        &key,
                        &value,
                        libbpf_rs::MapFlags::ANY,
                    )?;

                    debug!("Set IPv6 {addr} with protocol mask {protocols:#04x}");
                }
            }

            Ok(())
        }

        /// Sets the allowed protocol mask for a specific destination port.
        ///
        /// This overwrites any existing rules for this port.
        ///
        /// # Arguments
        ///
        /// * `port` - The destination port number.
        /// * `protocols` - Bitmask of allowed protocols.
        pub(crate) fn set_allowed_dst_port(
            &self,
            port: u16,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 2] = port.to_be_bytes();
            let value = PortRule {
                allowed_protocols: protocols,
                reserved: [0; 3],
            };
            // SAFETY: PortRule is POD (Plain Old Data) and #[repr(C)]
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

            debug!("Set port {port} with protocol mask {protocols:#04x}");

            Ok(())
        }

        /// Adds protocols to the allowed mask for a source IP.
        ///
        /// Performs a bitwise OR with the existing mask.
        pub(crate) fn add_allowed_src_ip(
            &self,
            addr: IpAddr,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let current_proto = self.get_allowed_src_ip_proto(addr)?;
            let new_proto = current_proto | protocols;
            self.set_allowed_src_ip(addr, new_proto)?;

            debug!(
                "Added protocols {protocols:#04x} to {addr}. Old: {current_proto:#04x}, New: {new_proto:#04x}"
            );

            Ok(())
        }

        /// Adds protocols to the allowed mask for a destination port.
        ///
        /// Performs a bitwise OR with the existing mask.
        pub(crate) fn add_allowed_dst_port(
            &self,
            port: u16,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let current_proto = self.get_allowed_dst_port_proto(port)?;
            let new_proto = current_proto | protocols;
            self.set_allowed_dst_port(port, new_proto)?;

            debug!(
                "Added protocols {protocols:#04x} to port {port}. Old: {current_proto:#04x}, New: {new_proto:#04x}"
            );

            Ok(())
        }

        /// Removes specific protocols from the allowed mask for a source IP.
        ///
        /// Performs a bitwise AND with the complement of the provided protocols.
        pub(crate) fn remove_allowed_src_ip(
            &self,
            addr: IpAddr,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let current_proto = self.get_allowed_src_ip_proto(addr)?;
            let new_proto = current_proto & !protocols;
            self.set_allowed_src_ip(addr, new_proto)?;

            debug!(
                "Removed protocols {protocols:#04x} from {addr}. Old: {current_proto:#04x}, New: {new_proto:#04x}"
            );

            Ok(())
        }

        /// Removes specific protocols from the allowed mask for a destination port.
        pub(crate) fn remove_allowed_dst_port(
            &self,
            port: u16,
            protocols: u8,
        ) -> Result<(), libbpf_rs::Error> {
            let current_proto = self.get_allowed_dst_port_proto(port)?;
            let new_proto = current_proto & !protocols;
            self.set_allowed_dst_port(port, new_proto)?;

            debug!(
                "Removed protocols {protocols:#04x} from port {port}. Old: {current_proto:#04x}, New: {new_proto:#04x}"
            );

            Ok(())
        }

        /// Retrieves the current protocol mask for a source IP.
        ///
        /// Returns `PROTO_NONE` (0) if no rule exists for the IP.
        pub(crate) fn get_allowed_src_ip_proto(
            &self,
            addr: IpAddr,
        ) -> Result<u8, libbpf_rs::Error> {
            match addr {
                IpAddr::V4(addr) => {
                    let key: [u8; 4] = addr.octets();
                    if let Some(value) = self
                        .skel
                        .maps
                        .allowed_src_ips_map_v4
                        .lookup(&key, libbpf_rs::MapFlags::ANY)?
                    {
                        Ok(value[0])
                    } else {
                        Ok(PROTO_NONE)
                    }
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    if let Some(value) = self
                        .skel
                        .maps
                        .allowed_src_ips_map_v6
                        .lookup(&key, libbpf_rs::MapFlags::ANY)?
                    {
                        Ok(value[0])
                    } else {
                        Ok(PROTO_NONE)
                    }
                }
            }
        }

        /// Retrieves the current protocol mask for a destination port.
        ///
        /// Returns `PROTO_NONE` (0) if no rule exists for the port.
        pub(crate) fn get_allowed_dst_port_proto(&self, port: u16) -> Result<u8, libbpf_rs::Error> {
            let key: [u8; 2] = port.to_be_bytes();
            if let Some(value) = self
                .skel
                .maps
                .allowed_dst_ports_map
                .lookup(&key, libbpf_rs::MapFlags::ANY)?
            {
                Ok(value[0])
            } else {
                Ok(PROTO_NONE)
            }
        }

        /// Deletes the rule for a specific source IP from the BPF map.
        pub(crate) fn delete_allowed_src_ip(&self, addr: IpAddr) -> Result<(), libbpf_rs::Error> {
            match addr {
                IpAddr::V4(addr) => {
                    let key: [u8; 4] = addr.octets();
                    self.skel.maps.allowed_src_ips_map_v4.delete(&key)?;
                    debug!("Deleted IPv4 {addr} from allowed source IPs");
                }
                IpAddr::V6(addr) => {
                    let key: [u8; 16] = addr.octets();
                    self.skel.maps.allowed_src_ips_map_v6.delete(&key)?;
                    debug!("Deleted IPv6 {addr} from allowed source IPs");
                }
            }

            Ok(())
        }

        /// Deletes the rule for a specific destination port from the BPF map.
        pub(crate) fn delete_allowed_dst_port(&self, port: u16) -> Result<(), libbpf_rs::Error> {
            let key: [u8; 2] = port.to_be_bytes();
            self.skel.maps.allowed_dst_ports_map.delete(&key)?;
            debug!("Deleted port {port} from allowed destination ports");

            Ok(())
        }
    }

    impl Drop for XdpFilter {
        fn drop(&mut self) {
            let xdp_attacher = libbpf_rs::Xdp::new(self.skel.progs.xdp_filter_prog.as_fd());
            // Attempt to detach, ignoring errors if it fails (e.g., if already detached)
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


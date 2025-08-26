#include <linux/types.h>

#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>
#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/tcp.h>
#include <linux/udp.h>

// AF_XDP 套接字映射
struct {
  __uint(type, BPF_MAP_TYPE_XSKMAP);
  __uint(key_size, sizeof(int));
  __uint(value_size, sizeof(int));
  __uint(max_entries, 64);
} xsks_map SEC(".maps");

// IPv4 源地址白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u32));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 1024);
} allowed_src_ips_map_v4 SEC(".maps");

// IPv6 源地址白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(struct in6_addr));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 1024);
} allowed_src_ips_map_v6 SEC(".maps");

// 目标端口白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u16));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 128);
} allowed_dst_ports_map SEC(".maps");

// 解析并检查 L4 (TCP/UDP) 头部
static __always_inline int check_l4_port(void *l4_header, void *data_end,
                                         __u8 protocol) {
  __u16 dst_port;

  if (protocol == IPPROTO_TCP) {
    struct tcphdr *tcp = l4_header;
    if ((void *)tcp + sizeof(*tcp) > data_end) {
      return XDP_PASS;
    }
    dst_port = tcp->dest;
  } else if (protocol == IPPROTO_UDP) {
    struct udphdr *udp = l4_header;
    if ((void *)udp + sizeof(*udp) > data_end) {
      return XDP_PASS;
    }
    dst_port = udp->dest;
  } else {
    return XDP_PASS; // 如果不是 TCP 或 UDP，直接放行
  }

  // 查询目标端口白名单
  if (bpf_map_lookup_elem(&allowed_dst_ports_map, &dst_port)) {
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  return XDP_PASS;
}

SEC("xdp")
int xdp_filter_prog(struct xdp_md *ctx) {
  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  struct ethhdr *eth = data;
  if ((void *)eth + sizeof(*eth) > data_end) {
    return XDP_PASS;
  }

  // 处理 ARP
  if (eth->h_proto == bpf_htons(ETH_P_ARP)) {
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  // --- IPv4 处理逻辑 ---
  if (eth->h_proto == bpf_htons(ETH_P_IP)) {
    struct iphdr *ip = data + sizeof(*eth);
    if ((void *)ip + sizeof(*ip) > data_end) {
      return XDP_PASS;
    }

    // 1. 检查源 IP 白名单 (Client 角色)
    if (bpf_map_lookup_elem(&allowed_src_ips_map_v4, &ip->saddr)) {
      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    void *l4_header = (void *)ip + sizeof(*ip);
    return check_l4_port(l4_header, data_end, ip->protocol);
  }

  // --- IPv6 处理逻辑 ---
  if (eth->h_proto == bpf_htons(ETH_P_IPV6)) {
    struct ipv6hdr *ip6 = data + sizeof(*eth);
    if ((void *)ip6 + sizeof(*ip6) > data_end) {
      return XDP_PASS;
    }

    // 1. 检查源 IP 白名单 (Client 角色)
    if (bpf_map_lookup_elem(&allowed_src_ips_map_v6, &ip6->saddr)) {
      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    void *l4_header = (void *)ip6 + sizeof(*ip6);
    return check_l4_port(l4_header, data_end, ip6->nexthdr);
  }

  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

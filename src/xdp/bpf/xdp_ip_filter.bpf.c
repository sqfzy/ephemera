#include <linux/types.h>

#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>
#include <linux/bpf.h>

#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/tcp.h>

// AF_XDP 套接字映射
struct {
  __uint(type, BPF_MAP_TYPE_XSKMAP);
  __uint(key_size, sizeof(int));
  __uint(value_size, sizeof(int));
  __uint(max_entries, 64);
} xsks_map SEC(".maps");

// IPv4 白名单哈希映射
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u32));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 1024);
} allowed_ips_map_v4 SEC(".maps");

// IPv6 白名单哈希映射
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(struct in6_addr));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 1024);
} allowed_ips_map_v6 SEC(".maps");

SEC("xdp")
int xdp_ip_filter_func(struct xdp_md *ctx) {
  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  struct ethhdr *eth = data;
  // 边界检查: 确保以太网头不会越界
  if ((void *)eth + sizeof(*eth) > data_end) {
    return XDP_PASS;
  }

  // 统一处理 ARP, 先直接重定向
  if (eth->h_proto == bpf_htons(ETH_P_ARP)) {
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  // --- IPv4 处理逻辑 ---
  if (eth->h_proto == bpf_htons(ETH_P_IP)) {
    struct iphdr *ip = data + sizeof(*eth);
    // 边界检查: 确保 IPv4 头不会越界
    if ((void *)ip + sizeof(*ip) > data_end) {
      return XDP_PASS;
    }

    // 暂时只处理 TCP 协议的白名单
    if (ip->protocol != IPPROTO_TCP) {
      return XDP_PASS;
    }

    // 查询 IPv4 白名单
    void *is_allowed = bpf_map_lookup_elem(&allowed_ips_map_v4, &ip->saddr);
    if (is_allowed) {
      // 在白名单中，重定向
      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    }

    // 不在白名单中，放行给内核
    return XDP_PASS;
  }

  // --- IPv6 处理逻辑 ---
  if (eth->h_proto == bpf_htons(ETH_P_IPV6)) {
    struct ipv6hdr *ip6 = data + sizeof(*eth);
    // 边界检查: 确保 IPv6 头不会越界
    if ((void *)ip6 + sizeof(*ip6) > data_end) {
      return XDP_PASS;
    }

    // `nexthdr` 字段类似于 IPv4 的 `protocol` 字段
    if (ip6->nexthdr != IPPROTO_TCP) {
      return XDP_PASS;
    }

    // 查询 IPv6 白名单, `ip6->saddr` 是一个 `struct in6_addr`
    void *is_allowed = bpf_map_lookup_elem(&allowed_ips_map_v6, &ip6->saddr);
    if (is_allowed) {
      // 在白名单中，重定向
      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    }

    // 不在白名单中，放行给内核
    return XDP_PASS;
  }

  // 如果不是 ARP, IPv4, 或 IPv6，直接放行
  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

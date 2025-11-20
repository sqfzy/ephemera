#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/ipv6.h>
#include <linux/types.h>

#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>

#include "parsing_helpers.h"

// ============================================================================
// 协议位掩码定义
// ============================================================================
#define PROTO_TCP (1 << 0)    // 0x01
#define PROTO_UDP (1 << 1)    // 0x02
#define PROTO_ICMP (1 << 2)   // 0x04
#define PROTO_ICMPV6 (1 << 3) // 0x08
#define PROTO_ALL (0xFF)      // 允许所有协议

// ============================================================================
// Map 定义
// ============================================================================

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
  __uint(value_size, sizeof(__u8)); // 协议位掩码
  __uint(max_entries, 1024);
} allowed_src_ips_map_v4 SEC(".maps");

// IPv6 源地址白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(struct in6_addr));
  __uint(value_size, sizeof(__u8)); // 协议位掩码
  __uint(max_entries, 1024);
} allowed_src_ips_map_v6 SEC(".maps");

// 目标端口白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u16));
  __uint(value_size, sizeof(__u8)); // 协议位掩码
  __uint(max_entries, 128);
} allowed_dst_ports_map SEC(".maps");

// ============================================================================
// 辅助函数
// ============================================================================

// 将 IP 协议号转换为位掩码
static __always_inline __u8 proto_to_mask(__u8 protocol) {
  switch (protocol) {
  case IPPROTO_TCP:
    return PROTO_TCP;
  case IPPROTO_UDP:
    return PROTO_UDP;
  case IPPROTO_ICMP:
    return PROTO_ICMP;
  case IPPROTO_ICMPV6:
    return PROTO_ICMPV6;
  default:
    return 0;
  }
}

// ============================================================================
// L4 端口检查
// ============================================================================

static __always_inline int check_l4_port(struct xdp_md *ctx,
                                         struct hdr_cursor *nh, void *data_end,
                                         __u8 protocol) {
  __u16 dst_port = 0;
  __u16 src_port = 0;

  // 获取协议对应的位掩码
  __u8 proto_mask = proto_to_mask(protocol);
  if (!proto_mask) {
    return XDP_PASS;
  }

  // 提取端口信息
  if (protocol == IPPROTO_TCP) {
    struct tcphdr *tcph;
    if (parse_tcphdr(nh, data_end, &tcph) < 0) {
      return XDP_PASS;
    }
    dst_port = tcph->dest;
    src_port = tcph->source;
  } else if (protocol == IPPROTO_UDP) {
    struct udphdr *udph;
    if (parse_udphdr(nh, data_end, &udph) < 0) {
      return XDP_PASS;
    }
    dst_port = udph->dest;
    src_port = udph->source;
  } else {
    return XDP_PASS; // ICMP 等协议没有端口概念
  }

  // 查询目标端口白名单
  __u8 *allowed_protocols =
      bpf_map_lookup_elem(&allowed_dst_ports_map, &dst_port);
  
  if (allowed_protocols ) {
    // 检查协议是否被允许
    if (*allowed_protocols  & proto_mask) {
      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    } else {
      // 端口匹配但协议不匹配，丢弃数据包
      return XDP_DROP;
    }
  }

  return XDP_PASS;
}

// ============================================================================
// 主程序
// ============================================================================

SEC("xdp")
int xdp_filter_prog(struct xdp_md *ctx) {
  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  struct hdr_cursor nh = {.pos = data};

  int eth_type;
  struct ethhdr *eth;

  // 解析以太网头
  eth_type = parse_ethhdr(&nh, data_end, &eth);
  if (eth_type < 0) {
    return XDP_PASS;
  }

  // 处理 ARP（始终重定向到用户空间）
  if (eth_type == bpf_htons(ETH_P_ARP)) {
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  // ========================================================================
  // IPv4 处理逻辑
  // ========================================================================
  if (eth_type == bpf_htons(ETH_P_IP)) {
    struct iphdr *iph;
    if (parse_iphdr(&nh, data_end, &iph) < 0) {
      return XDP_PASS;
    }

    __u8 proto_mask = proto_to_mask(iph->protocol);

    // 1. 检查源 IP 白名单 (Client 角色)
    __u8 *allowed_protos =
        bpf_map_lookup_elem(&allowed_src_ips_map_v4, &iph->saddr);
    
    if (allowed_protos) {
      if (*allowed_protos & proto_mask) {
        return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
      } else {
        return XDP_DROP;
      }
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    return check_l4_port(ctx, &nh, data_end, iph->protocol);
  }

  // ========================================================================
  // IPv6 处理逻辑
  // ========================================================================
  if (eth_type == bpf_htons(ETH_P_IPV6)) {
    struct ipv6hdr *ip6h;
    int proto;

    // 处理扩展头
    proto = parse_ip6hdr(&nh, data_end, &ip6h);
    if (proto < 0) {
      return XDP_DROP;
    }

    __u8 proto_mask = proto_to_mask(proto);

    // 1. 检查源 IP 白名单 (Client 角色)
    __u8 *allowed_protos =
        bpf_map_lookup_elem(&allowed_src_ips_map_v6, &ip6h->saddr);
    
    if (allowed_protos) {
      if (*allowed_protos & proto_mask) {
        return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
      } else {
        return XDP_DROP;
      }
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    return check_l4_port(ctx, &nh, data_end, proto);
  }

  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

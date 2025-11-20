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

// 日志级别
#define LOG_LEVEL_DEBUG 0
#define LOG_LEVEL_INFO 1
#define LOG_LEVEL_WARN 2
#define LOG_LEVEL_ERROR 3

// 日志事件类型
#define EVENT_PASS 1
#define EVENT_DROP 2
#define EVENT_REDIRECT 3
#define EVENT_PROTO_MISMATCH 4
#define EVENT_INVALID_PACKET 5

// ============================================================================
// 日志事件结构
// ============================================================================
struct log_event {
  __u64 timestamp;
  __u32 src_ip[4]; // 支持 IPv4/IPv6
  __u32 dst_ip[4];
  __u16 src_port;
  __u16 dst_port;
  __u8 protocol;
  __u8 ip_version;  // 4 or 6
  __u8 event_type;  // EVENT_*
  __u8 log_level;   // LOG_LEVEL_*
  char message[64]; // 日志消息
};

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

// 目标端口白名单结构
struct port_rule {
  __u8 allowed_protocols; // 协议位掩码
  __u8 reserved[3];       // 对齐填充
};

// 目标端口白名单
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u16));
  __uint(value_size, sizeof(struct port_rule));
  __uint(max_entries, 128);
} allowed_dst_ports_map SEC(".maps");

// Perf Event 数组（用于发送日志到用户空间）
struct {
  __uint(type, BPF_MAP_TYPE_PERF_EVENT_ARRAY);
  __uint(key_size, sizeof(__u32));
  __uint(value_size, sizeof(__u32));
} log_events SEC(".maps");

// 日志级别控制
struct {
  __uint(type, BPF_MAP_TYPE_ARRAY);
  __uint(key_size, sizeof(__u32));
  __uint(value_size, sizeof(__u8));
  __uint(max_entries, 1);
} log_level_map SEC(".maps");

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

// 字符串复制（BPF 安全版本）
static __always_inline void bpf_strcpy(char *dst, const char *src,
                                       int max_len) {
#pragma unroll
  for (int i = 0; i < max_len - 1; i++) {
    dst[i] = src[i];
    if (src[i] == '\0')
      break;
  }
  dst[max_len - 1] = '\0';
}

// 发送日志事件到用户空间
static __always_inline void log_event(struct xdp_md *ctx,
                                      struct log_event *event, __u8 level) {
  // 检查日志级别
  __u32 key = 0;
  __u8 *min_level = bpf_map_lookup_elem(&log_level_map, &key);
  if (min_level && level < *min_level) {
    return; // 日志级别不够，不记录
  }

  event->timestamp = bpf_ktime_get_ns();
  event->log_level = level;

  // 发送到 perf event
  bpf_perf_event_output(ctx, &log_events, BPF_F_CURRENT_CPU, event,
                        sizeof(*event));
}

// IPv6 地址复制
static __always_inline void copy_ipv6_addr(__u32 dst[4],
                                           const struct in6_addr *src) {
  dst[0] = src->in6_u.u6_addr32[0];
  dst[1] = src->in6_u.u6_addr32[1];
  dst[2] = src->in6_u.u6_addr32[2];
  dst[3] = src->in6_u.u6_addr32[3];
}

// ============================================================================
// L4 端口检查
// ============================================================================

static __always_inline int check_l4_port(struct xdp_md *ctx,
                                         struct hdr_cursor *nh, void *data_end,
                                         __u8 protocol,
                                         struct log_event *log_evt) {
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
      log_evt->event_type = EVENT_INVALID_PACKET;
      bpf_strcpy(log_evt->message, "TCP header truncated", 64);
      log_event(ctx, log_evt, LOG_LEVEL_WARN);
      return XDP_PASS;
    }
    dst_port = tcph->dest;
    src_port = tcph->source;
  } else if (protocol == IPPROTO_UDP) {
    struct udphdr *udph;
    if (parse_udphdr(nh, data_end, &udph) < 0) {
      log_evt->event_type = EVENT_INVALID_PACKET;
      bpf_strcpy(log_evt->message, "UDP header truncated", 64);
      log_event(ctx, log_evt, LOG_LEVEL_WARN);
      return XDP_PASS;
    }
    dst_port = udph->dest;
    src_port = udph->source;
  } else {
    return XDP_PASS; // ICMP 等协议没有端口概念
  }

  log_evt->src_port = bpf_ntohs(src_port);
  log_evt->dst_port = bpf_ntohs(dst_port);
  log_evt->protocol = protocol;

  // 查询目标端口白名单
  struct port_rule *rule =
      bpf_map_lookup_elem(&allowed_dst_ports_map, &dst_port);
  if (rule) {
    // 检查协议是否被允许
    if (rule->allowed_protocols & proto_mask) {
      log_evt->event_type = EVENT_REDIRECT;
      bpf_strcpy(log_evt->message, "Port matched, redirecting", 64);
      log_event(ctx, log_evt, LOG_LEVEL_DEBUG);

      return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    } else {
      // 端口匹配但协议不匹配，丢弃数据包
      log_evt->event_type = EVENT_PROTO_MISMATCH;
      bpf_strcpy(log_evt->message, "Port matched but protocol blocked", 64);
      log_event(ctx, log_evt, LOG_LEVEL_INFO);

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
  struct log_event log_evt = {0};

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

    log_evt.ip_version = 4;
    log_evt.src_ip[0] = iph->saddr;
    log_evt.dst_ip[0] = iph->daddr;
    log_evt.protocol = iph->protocol;

    __u8 proto_mask = proto_to_mask(iph->protocol);

    // 1. 检查源 IP 白名单 (Client 角色)
    __u8 *allowed_protos =
        bpf_map_lookup_elem(&allowed_src_ips_map_v4, &iph->saddr);
    if (allowed_protos) {
      if (*allowed_protos & proto_mask) {
        log_evt.event_type = EVENT_REDIRECT;
        bpf_strcpy(log_evt.message, "IPv4 src IP matched", 64);
        log_event(ctx, &log_evt, LOG_LEVEL_DEBUG);

        return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
      } else {
        log_evt.event_type = EVENT_PROTO_MISMATCH;
        bpf_strcpy(log_evt.message, "IPv4 src IP matched but protocol blocked",
                   64);
        log_event(ctx, &log_evt, LOG_LEVEL_INFO);

        return XDP_DROP;
      }
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    return check_l4_port(ctx, &nh, data_end, iph->protocol, &log_evt);
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
      log_evt.event_type = EVENT_INVALID_PACKET;
      log_evt.ip_version = 6;
      bpf_strcpy(log_evt.message, "IPv6 header parse failed", 64);
      log_event(ctx, &log_evt, LOG_LEVEL_ERROR);
      return XDP_DROP;
    }

    log_evt.ip_version = 6;
    copy_ipv6_addr(log_evt.src_ip, &ip6h->saddr);
    copy_ipv6_addr(log_evt.dst_ip, &ip6h->daddr);
    log_evt.protocol = proto;

    __u8 proto_mask = proto_to_mask(proto);

    // 1. 检查源 IP 白名单 (Client 角色)
    __u8 *allowed_protos =
        bpf_map_lookup_elem(&allowed_src_ips_map_v6, &ip6h->saddr);
    if (allowed_protos) {
      if (*allowed_protos & proto_mask) {
        log_evt.event_type = EVENT_REDIRECT;
        bpf_strcpy(log_evt.message, "IPv6 src IP matched", 64);
        log_event(ctx, &log_evt, LOG_LEVEL_DEBUG);

        return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
      } else {
        log_evt.event_type = EVENT_PROTO_MISMATCH;
        bpf_strcpy(log_evt.message, "IPv6 src IP matched but protocol blocked",
                   64);
        log_event(ctx, &log_evt, LOG_LEVEL_INFO);

        return XDP_DROP;
      }
    }

    // 2. 检查目标端口白名单 (Listener 角色)
    return check_l4_port(ctx, &nh, data_end, proto, &log_evt);
  }

  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

#include <linux/types.h>

#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>
#include <linux/bpf.h>

#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/tcp.h>

// AF_XDP 套接字映射
struct {
  __uint(type, BPF_MAP_TYPE_XSKMAP);
  __uint(key_size, sizeof(int));
  __uint(value_size, sizeof(int));
  __uint(max_entries, 64);
} xsks_map SEC(".maps");

// IP 白名单哈希映射
//    我们用这个哈希表来存储所有允许的源 IP 地址。
//    Key:   IPv4 地址 (网络字节序)
//    Value: 一个简单的标志位 (例如 u8)，我们只关心 Key 是否存在。
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u32));  // Key 是 IPv4 地址
  __uint(value_size, sizeof(__u8)); // Value 只是一个存在性标志
  __uint(max_entries, 1024);        // 最多允许 1024 个白名单 IP
} allowed_ips_map SEC(".maps");

// --- BPF 程序主体 ---

SEC("xdp")
int xdp_ip_filter_func(struct xdp_md *ctx) {
  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  struct ethhdr *eth = data;
  if ((void *)eth + sizeof(*eth) > data_end) {
    return XDP_PASS;
  }

  // 转发ARP包
  if (eth->h_proto == bpf_htons(ETH_P_ARP)) {
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  if (eth->h_proto != bpf_htons(ETH_P_IP)) {
    return XDP_PASS;
  }

  struct iphdr *ip = data + sizeof(*eth);
  if ((void *)ip + sizeof(*ip) > data_end) {
    return XDP_PASS;
  }

  if (ip->protocol != IPPROTO_TCP) {
    return XDP_PASS;
  }

  // 检查 IP 是否在白名单
  void *is_allowed = bpf_map_lookup_elem(&allowed_ips_map, &ip->saddr);

  if (is_allowed) {
    // IP 在白名单中，重定向数据包到队列0 (Rust程序必须监听队列0)。
    return bpf_redirect_map(&xsks_map, 0, XDP_PASS);
  }

  // 没查到，放行给内核。
  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

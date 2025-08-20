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
//    Key:   IPv4 地址 (网络字节序)
//    Value: 一个简单的标志位 (例如 u8)，我们只关心 Key 是否存在。
struct {
  __uint(type, BPF_MAP_TYPE_HASH);
  __uint(key_size, sizeof(__u32));  // Key 是 IPv4 地址
  __uint(value_size, sizeof(__u8)); // Value 只是一个存在性标志
  __uint(max_entries, 1024);        // 最多允许 1024 个白名单 IP
} allowed_ips_map SEC(".maps");

SEC("xdp")
int xdp_ip_filter_func(struct xdp_md *ctx) {
  // debug
  {
    int queue_id = 0;
    int *socket_fd_ptr;

    // 从 map 中查找值
    socket_fd_ptr = bpf_map_lookup_elem(&xsks_map, &queue_id);

    if (socket_fd_ptr) {
      // 如果找到了，就打印出来
      // %d 会被替换成 socket_fd_ptr 指向的值
      bpf_printk("XDP: Found fd %d for queue_id %d\n", *socket_fd_ptr,
                 queue_id);
    } else {
      bpf_printk("XDP: No fd found for queue_id %d\n", queue_id);
    }
  }

  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  struct ethhdr *eth = data;
  if ((void *)eth + sizeof(*eth) > data_end) {
    bpf_printk("PASS. length too long");
    return XDP_PASS;
  }

  // 转发ARP包
  if (eth->h_proto == bpf_htons(ETH_P_ARP)) {
    long res = bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    bpf_printk("debug REDIRECT APR, res=%d", res);
    return res;
  }

  if (eth->h_proto != bpf_htons(ETH_P_IP)) {
    bpf_printk("PASS. not IP packet");
    return XDP_PASS;
  }

  struct iphdr *ip = data + sizeof(*eth);
  if ((void *)ip + sizeof(*ip) > data_end) {
    bpf_printk("PASS. IP length too long");
    return XDP_PASS;
  }

  if (ip->protocol != IPPROTO_TCP) {
    bpf_printk("PASS. not TCP packet");
    return XDP_PASS;
  }

  // 检查 IP 是否在白名单
  void *is_allowed = bpf_map_lookup_elem(&allowed_ips_map, &ip->saddr);
  bpf_printk("debug receive TCP, saddr=%x, is_allowed=%p", ip->saddr, is_allowed);

  if (is_allowed) {
    // IP 在白名单中，重定向数据包到队列0 (Rust程序必须监听队列0)。
    long res = bpf_redirect_map(&xsks_map, 0, XDP_PASS);
    bpf_printk("debug REDIRECT TCP, res=%d", res);
    return res;
  }

  bpf_printk("debug receive TCP, but not in whitelist");
  // 没查到，放行给内核。
  return XDP_PASS;
}

char _license[] SEC("license") = "GPL";

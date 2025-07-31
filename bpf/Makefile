# Makefile
CC = clang
CFLAGS = -g -O2

TARGET_XDP = xdp_fwd.o

# 编译 XDP 程序。关键是 -target bpf
bpf: $(TARGET_XDP)

$(TARGET_XDP): xdp_fwd.c
	$(CC) $(CFLAGS) -target bpf -c $< -o $@

clean:
	rm -f $(TARGET_XDP)

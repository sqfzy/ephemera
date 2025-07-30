# AF_XDP Device Integration Tests

This directory contains integration tests for the `src/af_xdp/device.rs` module, which implements an AF_XDP-based network device for the smoltcp library.

## Overview

The tests verify that the `XdpDevice` implementation correctly:
- Creates XDP sockets on network interfaces
- Implements the smoltcp `Device` trait
- Handles packet transmission and reception
- Manages memory and file descriptors properly
- Works in isolated network environments

## Test Structure

### Basic Tests
- `test_prerequisites` - Verifies test environment setup
- `test_device_creation_invalid_interface` - Tests error handling for invalid interfaces
- `test_device_creation_direct` - Tests device creation

### Integration Tests (Require Network Setup)
- `test_device_functionality` - Tests basic device operations
- `test_packet_transmission` - Tests packet creation and transmission
- `test_device_packet_handling` - Tests packet reception
- `test_device_stress` - Stress tests device operations

### Advanced Tests
- `test_ping_packet_handling` - Tests with real ping traffic
- `test_arp_traffic_generation` - Tests ARP packet handling

## Network Environment

The tests use a virtual network environment created by `setup_network.sh`:

```
Host Network Namespace          Test Network Namespace (xdp_test_ns)
┌─────────────────────┐        ┌─────────────────────┐
│                     │        │                     │
│   veth_host         │←──────→│   veth_guest        │
│   192.168.100.1/24  │        │   192.168.100.2/24  │
│                     │        │                     │
└─────────────────────┘        └─────────────────────┘
         │                              │
         │                              │
     Internet ←─────── NAT ─────────────┘
```

### Network Configuration
- **Namespace**: `xdp_test_ns`
- **Host interface**: `veth_host` (192.168.100.1/24)
- **Guest interface**: `veth_guest` (192.168.100.2/24) - used for XDP
- **Connectivity**: NAT provides internet access to the test namespace

## Running Tests

### Quick Start
```bash
# Run basic tests (no sudo required)
cargo test

# Run all tests including integration tests
./run_tests.sh test-all
```

### Step by Step
```bash
# 1. Set up network environment
./run_tests.sh setup

# 2. Run basic tests
./run_tests.sh test

# 3. Run integration tests (requires sudo)
./run_tests.sh test-sudo

# 4. Clean up
./run_tests.sh teardown
```

### Manual Test Execution
```bash
# Basic tests
cargo test test_prerequisites
cargo test test_device_creation_invalid_interface

# Integration tests (requires network setup)
cargo test -- --ignored

# Specific test in namespace
sudo ip netns exec xdp_test_ns cargo test test_device_functionality
```

## Requirements

### System Requirements
- Linux kernel with AF_XDP support
- Root/sudo access for network namespace operations
- Required packages: `libelf-dev`, `libpcap-dev`

### Tools Required
- `ip` (iproute2)
- `iptables`
- `cargo` (Rust toolchain)
- Optional: `arping` for ARP traffic generation

### Rust Dependencies
The tests use these crates:
- `smoltcp` - Network protocol stack
- `xsk-rs` - AF_XDP bindings
- Standard library for process management and threading

## Test Implementation Details

### XdpDevice Testing Strategy

1. **Creation Testing**: Verify device can be created on valid interfaces and fails appropriately on invalid ones.

2. **Interface Testing**: Test the smoltcp `Device` trait implementation:
   - `capabilities()` returns correct MTU and medium type
   - `transmit()` and `receive()` return appropriate tokens
   - Raw file descriptor access works

3. **Packet Handling**: Test actual packet creation, transmission, and reception using the AF_XDP interface.

4. **Integration Testing**: Use real network traffic (ping, ARP) to verify end-to-end functionality.

### Network Namespace Usage

Tests that require actual network interfaces run inside the `xdp_test_ns` namespace to:
- Isolate test traffic from the host system
- Provide a known network configuration
- Allow testing without affecting production interfaces
- Enable testing of XDP socket binding

### Error Handling

The tests are designed to gracefully handle various error conditions:
- Missing network setup (tests are skipped with informative messages)
- Insufficient permissions (clear error messages)
- AF_XDP not supported (tests report the limitation)

## Debugging

### Check Network Status
```bash
./run_tests.sh status
```

### Manual Network Verification
```bash
# Check namespace exists
ip netns list | grep xdp_test_ns

# Check interface in namespace
sudo ip netns exec xdp_test_ns ip link show veth_guest

# Test connectivity
sudo ip netns exec xdp_test_ns ping -c 2 192.168.100.1
```

### Test Debugging
```bash
# Run with verbose output
cargo test -- --nocapture

# Run specific test with debugging
RUST_LOG=debug cargo test test_device_functionality -- --nocapture
```

## Limitations

1. **Permissions**: Integration tests require sudo access for network namespace operations.

2. **AF_XDP Support**: Tests require kernel and hardware support for AF_XDP. On systems without support, device creation will fail (which is expected behavior).

3. **Concurrency**: Network namespace tests should not be run concurrently as they share the same namespace.

4. **Platform**: Tests are Linux-specific due to AF_XDP and network namespace requirements.

## Contributing

When adding new tests:

1. **Basic tests** should not require network setup or special permissions
2. **Integration tests** should be marked with `#[ignore]` and handle missing network setup gracefully
3. **Advanced tests** should include proper cleanup and error handling
4. Update this documentation with new test scenarios

Example test pattern:
```rust
#[test]
#[ignore] // Mark as integration test
fn test_new_feature() {
    let device = match XdpDevice::new("veth_guest") {
        Ok(dev) => dev,
        Err(_) => {
            println!("Skipping test - not in namespace");
            return;
        }
    };
    
    // Test implementation here
}
```
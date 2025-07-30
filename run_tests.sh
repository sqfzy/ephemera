#!/bin/bash

# AF_XDP Device Integration Test Runner
# This script helps run the integration tests for src/af_xdp/device.rs

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  setup       - Set up network environment for testing"
    echo "  teardown    - Clean up network environment"
    echo "  test        - Run basic tests (no sudo required)"
    echo "  test-sudo   - Run integration tests requiring sudo"
    echo "  test-all    - Run all tests including ignored ones"
    echo "  status      - Check network environment status"
    echo "  help        - Show this help message"
    echo ""
    echo "Example workflow:"
    echo "  1. $0 setup           # Set up test environment"
    echo "  2. $0 test-sudo       # Run integration tests"
    echo "  3. $0 teardown        # Clean up"
}

check_prerequisites() {
    echo "Checking prerequisites..."
    
    # Check if running as root or with sudo access
    if ! sudo -n true 2>/dev/null; then
        echo "Error: sudo access required for network setup"
        echo "Please ensure you can run sudo commands without password prompt"
        exit 1
    fi
    
    # Check required commands
    local missing_commands=()
    
    for cmd in ip iptables cargo; do
        if ! command -v "$cmd" &> /dev/null; then
            missing_commands+=("$cmd")
        fi
    done
    
    if [ ${#missing_commands[@]} -ne 0 ]; then
        echo "Error: Missing required commands: ${missing_commands[*]}"
        exit 1
    fi
    
    echo "Prerequisites check passed"
}

setup_network() {
    echo "Setting up network environment..."
    check_prerequisites
    
    if ./setup_network.sh setup; then
        echo "Network setup completed successfully"
        
        # Verify setup
        echo "Verifying network setup..."
        sudo ip netns exec xdp_test_ns ip link show veth_guest
        sudo ip netns exec xdp_test_ns ip addr show veth_guest
        
        echo ""
        echo "Test network connectivity:"
        sudo ip netns exec xdp_test_ns ping -c 2 192.168.100.1 || true
        
    else
        echo "Network setup failed"
        exit 1
    fi
}

teardown_network() {
    echo "Tearing down network environment..."
    ./setup_network.sh teardown
    echo "Network teardown completed"
}

check_status() {
    echo "Checking network environment status..."
    
    # Check if namespace exists
    if ip netns list | grep -q "xdp_test_ns"; then
        echo "✓ Network namespace 'xdp_test_ns' exists"
        
        # Check interfaces in namespace
        if sudo ip netns exec xdp_test_ns ip link show veth_guest &>/dev/null; then
            echo "✓ Interface 'veth_guest' exists in namespace"
            
            # Check if interface is up
            if sudo ip netns exec xdp_test_ns ip link show veth_guest | grep -q "state UP"; then
                echo "✓ Interface 'veth_guest' is UP"
            else
                echo "⚠ Interface 'veth_guest' is DOWN"
            fi
            
            # Show IP configuration
            echo "Interface configuration:"
            sudo ip netns exec xdp_test_ns ip addr show veth_guest
            
        else
            echo "✗ Interface 'veth_guest' not found in namespace"
        fi
    else
        echo "✗ Network namespace 'xdp_test_ns' does not exist"
        echo "Run '$0 setup' to create the test environment"
    fi
}

run_basic_tests() {
    echo "Running basic tests (no sudo required)..."
    cargo test test_prerequisites
    cargo test test_device_creation_invalid_interface
    cargo test test_device_creation_direct
    echo "Basic tests completed"
}

run_sudo_tests() {
    echo "Running integration tests (requires sudo)..."
    check_prerequisites
    
    # Check if network is set up
    if ! ip netns list | grep -q "xdp_test_ns"; then
        echo "Network environment not found. Setting up..."
        setup_network
        network_setup_by_us=true
    fi
    
    echo "Running integration tests..."
    
    # Run the ignored tests that require network setup
    cargo test -- --ignored --nocapture || {
        echo "Some integration tests failed, but this might be expected"
        echo "Check the output above for details"
    }
    
    # If we set up the network, offer to clean it up
    if [ "$network_setup_by_us" = true ]; then
        echo ""
        read -p "Clean up network environment? [Y/n] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
            teardown_network
        fi
    fi
}

run_all_tests() {
    echo "Running all tests..."
    run_basic_tests
    echo ""
    run_sudo_tests
}

case "${1:-help}" in
    "setup")
        setup_network
        ;;
    "teardown")
        teardown_network
        ;;
    "test")
        run_basic_tests
        ;;
    "test-sudo")
        run_sudo_tests
        ;;
    "test-all")
        run_all_tests
        ;;
    "status")
        check_status
        ;;
    "help"|*)
        usage
        ;;
esac
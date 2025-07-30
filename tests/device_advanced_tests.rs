use af_xdp_ws::af_xdp::device::XdpDevice;
use std::process::Command;
use std::os::fd::AsRawFd;
use smoltcp::time::Instant;
use smoltcp::phy::{Device, TxToken, RxToken};
use std::thread;
use std::time::{Duration, Instant as StdInstant};

/// Helper to run a command in the test network namespace
fn run_in_namespace(cmd: &str, args: &[&str]) -> Result<std::process::Output, Box<dyn std::error::Error>> {
    let mut command = Command::new("sudo");
    command
        .arg("ip")
        .arg("netns")
        .arg("exec")
        .arg("xdp_test_ns")
        .arg(cmd);
    
    for arg in args {
        command.arg(arg);
    }
    
    let output = command.output()?;
    Ok(output)
}

/// Test using ping to generate and receive packets through the XDP device
#[test]
#[ignore] // Requires network setup and sudo
fn test_ping_packet_handling() {
    // Verify namespace exists and veth_guest is available
    let output = run_in_namespace("ip", &["link", "show", "veth_guest"])
        .expect("Failed to check veth_guest interface");
    
    if !output.status.success() {
        panic!("veth_guest interface not found. Run './setup_network.sh setup' first");
    }
    
    // Start a ping in the background to generate packets
    let ping_handle = thread::spawn(|| {
        thread::sleep(Duration::from_secs(1)); // Give device time to start
        
        let _output = run_in_namespace("ping", &["-c", "3", "-i", "0.5", "192.168.100.1"])
            .expect("Failed to run ping");
    });
    
    // Run packet handling test in namespace
    let output = run_in_namespace(
        "cargo",
        &["test", "--", "test_device_packet_handling", "--nocapture"]
    ).expect("Failed to run packet handling test");
    
    ping_handle.join().expect("Ping thread failed");
    
    if !output.status.success() {
        panic!("Packet handling test failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    println!("Ping packet handling test completed successfully");
}

/// Test device packet handling (to be run in namespace)
#[test] 
fn test_device_packet_handling() {
    let mut device = match XdpDevice::<1024>::new("veth_guest") {
        Ok(dev) => dev,
        Err(e) => {
            println!("Skipping packet handling test - not in namespace: {}", e);
            return;
        }
    };
    
    println!("XDP device created successfully");
    
    let start_time = StdInstant::now();
    let mut packet_count = 0;
    let test_duration = Duration::from_secs(5);
    
    while start_time.elapsed() < test_duration {
        let now = Instant::now();
        
        // Check for received packets
        if let Some((rx_token, _tx_token)) = device.receive(now) {
            packet_count += 1;
            
            // Process the received packet
            rx_token.consume(|packet_data| {
                println!("Received packet {} with {} bytes", packet_count, packet_data.len());
                
                // Basic validation - check if it looks like an Ethernet frame
                if packet_data.len() >= 14 {
                    let ethertype = u16::from_be_bytes([packet_data[12], packet_data[13]]);
                    println!("  EtherType: 0x{:04x}", ethertype);
                    
                    match ethertype {
                        0x0800 => println!("  IPv4 packet"),
                        0x0806 => println!("  ARP packet"),
                        0x86dd => println!("  IPv6 packet"),
                        _ => println!("  Other/Unknown packet type"),
                    }
                }
                
                packet_count
            });
        }
        
        // Small delay to avoid busy-waiting
        thread::sleep(Duration::from_millis(10));
    }
    
    println!("Processed {} packets in test duration", packet_count);
    
    // Test is successful if we can create the device and don't crash
    // Packet count might be 0 if no traffic is flowing, which is okay
}

/// Test device with ARP traffic generation
#[test]
#[ignore] // Requires network setup and sudo
fn test_arp_traffic_generation() {
    // Verify namespace and generate ARP traffic
    let output = run_in_namespace("ip", &["link", "show", "veth_guest"])
        .expect("Failed to check veth_guest interface");
    
    if !output.status.success() {
        panic!("veth_guest interface not found. Run './setup_network.sh setup' first");
    }
    
    // Generate ARP traffic to test packet handling
    let arp_handle = thread::spawn(|| {
        thread::sleep(Duration::from_secs(1));
        
        // Send ARP requests to generate traffic
        let _output = run_in_namespace("arping", &["-c", "3", "-I", "veth_guest", "192.168.100.1"])
            .unwrap_or_else(|_| {
                // If arping is not available, try ping instead
                run_in_namespace("ping", &["-c", "2", "192.168.100.1"])
                    .expect("Failed to generate traffic")
            });
    });
    
    // Run ARP handling test in namespace
    let output = run_in_namespace(
        "cargo",
        &["test", "--", "test_device_arp_handling", "--nocapture"]
    ).expect("Failed to run ARP handling test");
    
    arp_handle.join().expect("ARP traffic thread failed");
    
    if !output.status.success() {
        panic!("ARP handling test failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Test ARP packet handling specifically (to be run in namespace)
#[test]
fn test_device_arp_handling() {
    let mut device = match XdpDevice::<1024>::new("veth_guest") {
        Ok(dev) => dev,
        Err(e) => {
            println!("Skipping ARP handling test - not in namespace: {}", e);
            return;
        }
    };
    
    println!("Testing ARP packet handling");
    
    let start_time = StdInstant::now();
    let mut arp_packet_count = 0;
    let test_duration = Duration::from_secs(3);
    
    while start_time.elapsed() < test_duration {
        let now = Instant::now();
        
        if let Some((rx_token, _tx_token)) = device.receive(now) {
            rx_token.consume(|packet_data| {
                if packet_data.len() >= 14 {
                    let ethertype = u16::from_be_bytes([packet_data[12], packet_data[13]]);
                    
                    if ethertype == 0x0806 { // ARP packet
                        arp_packet_count += 1;
                        println!("Received ARP packet #{}", arp_packet_count);
                        
                        if packet_data.len() >= 42 {
                            let opcode = u16::from_be_bytes([packet_data[20], packet_data[21]]);
                            match opcode {
                                1 => println!("  ARP Request"),
                                2 => println!("  ARP Reply"),
                                _ => println!("  Unknown ARP opcode: {}", opcode),
                            }
                        }
                    }
                }
                arp_packet_count
            });
        }
        
        thread::sleep(Duration::from_millis(10));
    }
    
    println!("Processed {} ARP packets", arp_packet_count);
}

/// Test device performance and stress scenarios
#[test]
fn test_device_stress() {
    let mut device = match XdpDevice::<1024>::new("veth_guest") {
        Ok(dev) => dev,
        Err(_) => {
            println!("Skipping stress test - not in namespace");
            return;
        }
    };
    
    println!("Running device stress test");
    
    let start_time = StdInstant::now();
    let test_duration = Duration::from_secs(2);
    let mut operations = 0;
    
    // Stress test: repeatedly try to get tokens and check capabilities
    while start_time.elapsed() < test_duration {
        let now = Instant::now();
        
        // Test transmit token acquisition
        if let Some(_tx_token) = device.transmit(now) {
            // Don't actually use the token, just test acquisition
        }
        
        // Test receive token acquisition  
        if let Some((_rx_token, _tx_token)) = device.receive(now) {
            // Don't actually use tokens, just test acquisition
        }
        
        // Test capabilities (should be fast)
        let _caps = device.capabilities();
        
        // Test raw FD access
        let _fd = device.as_raw_fd();
        
        operations += 1;
    }
    
    println!("Completed {} operations in {} seconds", 
             operations, test_duration.as_secs());
    
    // Verify we can still create packets after stress test
    let now = Instant::now();
    if let Some(tx_token) = device.transmit(now) {
        tx_token.consume(64, |buffer| {
            buffer[0..6].fill(0xff); // broadcast
            64
        });
        
        // Try to wake up kernel
        if let Err(e) = device.wakeup_kernel() {
            println!("Warning: Failed to wake up kernel after stress test: {}", e);
        }
    }
    
    println!("Device stress test completed successfully");
}
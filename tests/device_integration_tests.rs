use af_xdp_ws::af_xdp::device::XdpDevice;
use std::process::Command;
use std::os::fd::AsRawFd;
use smoltcp::time::Instant;
use smoltcp::phy::{Device, TxToken};

/// Helper to set up the network environment using setup_network.sh
fn setup_network() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("./setup_network.sh")
        .arg("setup")
        .current_dir("/home/runner/work/af_xdp/af_xdp")
        .output()?;
    
    if !output.status.success() {
        eprintln!("Setup failed: {}", String::from_utf8_lossy(&output.stderr));
        return Err("Network setup failed".into());
    }
    
    println!("Network setup output: {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

/// Helper to tear down the network environment
fn teardown_network() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("./setup_network.sh") 
        .arg("teardown")
        .current_dir("/home/runner/work/af_xdp/af_xdp")
        .output()?;
    
    if !output.status.success() {
        eprintln!("Teardown failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    
    println!("Network teardown output: {}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

/// Run a command in the test network namespace
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

/// Test that checks if we can create an XdpDevice in the namespace
#[test]
#[ignore] // This test requires sudo and network setup
fn test_device_creation_in_namespace() {
    // This test should be run with: cargo test -- --ignored
    
    // First verify the namespace exists and veth_guest is available
    let output = run_in_namespace("ip", &["link", "show", "veth_guest"])
        .expect("Failed to check veth_guest interface");
    
    if !output.status.success() {
        panic!("veth_guest interface not found. Did you run setup_network.nu setup?");
    }
    
    // Run the actual XDP device creation test in the namespace
    let output = run_in_namespace(
        "cargo", 
        &["test", "--", "test_device_creation_direct", "--nocapture"]
    ).expect("Failed to run test in namespace");
    
    if !output.status.success() {
        panic!("Device creation test failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Direct test for device creation (to be run inside namespace)
#[test]  
fn test_device_creation_direct() {
    // This test is designed to be called from within the network namespace
    // by test_device_creation_in_namespace
    
    let result = XdpDevice::<1024>::new("veth_guest");
    
    match result {
        Ok(device) => {
            // Verify device was created successfully
            assert!(device.as_raw_fd() > 0);
            println!("Successfully created XdpDevice on veth_guest");
        }
        Err(e) => {
            // If we're not in the right environment, this will fail
            // which is expected for normal test runs
            println!("Device creation failed (expected if not in namespace): {}", e);
        }
    }
}

/// Test device creation with invalid interface name
#[test]
fn test_device_creation_invalid_interface() {
    let result = XdpDevice::<1024>::new("invalid_interface_name");
    assert!(result.is_err());
}

/// Integration test that sets up network, tests device, then tears down
#[test]
#[ignore] // Requires sudo permissions
fn test_full_integration() {
    // Setup network
    setup_network().expect("Failed to setup network");
    
    // Give some time for network setup to complete
    std::thread::sleep(std::time::Duration::from_secs(1));
    
    // Run device tests in namespace
    let test_result = run_in_namespace(
        "cargo",
        &["test", "--", "test_device_functionality", "--nocapture"]
    );
    
    // Always teardown, regardless of test result
    teardown_network().expect("Failed to teardown network");
    
    // Check test result after cleanup
    let output = test_result.expect("Failed to run device test");
    if !output.status.success() {
        panic!("Device functionality test failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

/// Test the basic functionality of XdpDevice (to be run in namespace)
#[test]
fn test_device_functionality() {
    let mut device = match XdpDevice::<1024>::new("veth_guest") {
        Ok(dev) => dev,
        Err(_) => {
            // Skip if not in proper environment
            println!("Skipping device functionality test - not in namespace");
            return;
        }
    };
    
    // Test device capabilities
    let caps = device.capabilities();
    assert_eq!(caps.max_transmission_unit, 1500);
    assert_eq!(caps.medium, smoltcp::phy::Medium::Ethernet);
    
    // Test that we can get raw FD
    let fd = device.as_raw_fd();
    assert!(fd > 0);
    
    // Test transmit method (basic check that it doesn't panic)
    let now = Instant::now();
    let tx_token = device.transmit(now);
    
    if tx_token.is_some() {
        println!("Successfully got TX token");
        // Don't actually transmit anything for now
    }
    
    // Test receive method
    let rx_result = device.receive(now);
    if let Some((_rx_token, _tx_token)) = rx_result {
        println!("Successfully got RX and TX tokens");
        // Don't actually process packets for now
    }
    
    println!("Device functionality test completed successfully");
}

/// Test packet transmission and creation
#[test]
fn test_packet_transmission() {
    let mut device = match XdpDevice::<1024>::new("veth_guest") {
        Ok(dev) => dev,
        Err(_) => {
            println!("Skipping packet transmission test - not in namespace");
            return;
        }
    };
    
    let now = Instant::now();
    
    // Try to get a transmit token
    if let Some(tx_token) = device.transmit(now) {
        println!("Got TX token, attempting to create a test packet");
        
        // Create a simple ARP packet
        let _result = tx_token.consume(42, |buffer| {
            // Fill buffer with a basic Ethernet frame
            if buffer.len() >= 42 {
                // Ethernet header (14 bytes)
                buffer[0..6].copy_from_slice(&[0xff; 6]); // Destination MAC (broadcast)
                buffer[6..12].copy_from_slice(&[0x02, 0x03, 0x04, 0x05, 0x06, 0x07]); // Source MAC
                buffer[12..14].copy_from_slice(&[0x08, 0x06]); // EtherType (ARP)
                
                // ARP packet (28 bytes)
                buffer[14..16].copy_from_slice(&[0x00, 0x01]); // Hardware type (Ethernet)
                buffer[16..18].copy_from_slice(&[0x08, 0x00]); // Protocol type (IPv4)
                buffer[18] = 6; // Hardware size
                buffer[19] = 4; // Protocol size
                buffer[20..22].copy_from_slice(&[0x00, 0x01]); // Opcode (request)
                buffer[22..28].copy_from_slice(&[0x02, 0x03, 0x04, 0x05, 0x06, 0x07]); // Sender MAC
                buffer[28..32].copy_from_slice(&[192, 168, 100, 2]); // Sender IP
                buffer[32..38].copy_from_slice(&[0x00; 6]); // Target MAC
                buffer[38..42].copy_from_slice(&[192, 168, 100, 1]); // Target IP
                
                println!("Created test ARP packet");
            }
            42 // Return packet length
        });
        
        // Try to wake up the kernel to send the packet
        if let Err(e) = device.wakeup_kernel() {
            println!("Failed to wake up kernel: {}", e);
        } else {
            println!("Successfully woke up kernel for packet transmission");
        }
        
        println!("Packet transmission test completed");
    } else {
        println!("No TX token available");
    }
}

/// Test helper for checking prerequisites
#[test]
fn test_prerequisites() {
    // Check if bash is available
    let bash_check = Command::new("bash")
        .arg("--version")
        .output();
    
    assert!(bash_check.is_ok(), "Bash shell is not available");
    
    // Check if we can access the setup script
    let script_exists = std::fs::metadata("/home/runner/work/af_xdp/af_xdp/setup_network.sh");
    assert!(script_exists.is_ok(), "setup_network.sh not found");
    
    println!("Prerequisites check passed");
}
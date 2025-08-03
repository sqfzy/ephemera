use ephemera::af_xdp::{listener::XdpTcpListener, stream::XdpTcpStream};
use std::{
    io::{Read, Write},
    process::Command,
    sync::LazyLock,
    thread,
};

// Test constants
const INTERFACE_NAME1: &str = "test_iface1";
const INTERFACE_IP1: &str = "192.168.10.1";

const INTERFACE_NAME2: &str = "test_iface2";

struct SetupGuard;

impl Drop for SetupGuard {
    fn drop(&mut self) {
        Command::new("sudo")
            .arg("nu")
            .arg("tests/cleanup_net.nu")
            .status()
            .expect("Failed to execute cleanup_net.nu script");
    }
}

static SETUP: LazyLock<SetupGuard> = LazyLock::new(|| {
    // Allow all log levels
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    // Run the setup script
    let status = Command::new("sudo")
        .arg("nu")
        .arg("tests/setup_net.nu")
        .status()
        .expect("Failed to execute setup_net.nu script");
    assert!(status.success(), "setup_net.nu script failed");

    SetupGuard
});

#[test]
fn test_listener_accept_and_stream() {
    let _ = &*SETUP;

    let server_addr = format!("{}:8080", INTERFACE_IP1);
    let server_addr_clone = server_addr.clone();

    let server_thread = thread::spawn(move || {
        let listener = XdpTcpListener::bind(server_addr_clone).unwrap();
        let (mut stream, _new_listener) = listener.accept().unwrap();

        let mut buf = [0; 10];
        let bytes_read = stream.read(&mut buf).unwrap();

        assert_eq!(bytes_read, 5);
        assert_eq!(&buf[..bytes_read], b"hello");

        stream.write_all(b"world").unwrap();
        stream.flush().unwrap();
    });

    // Give the server a moment to start listening
    thread::sleep(std::time::Duration::from_secs(1));

    let client_thread = thread::spawn(move || {
        let mut stream = XdpTcpStream::connect(server_addr).unwrap();
        stream.write_all(b"hello").unwrap();
        stream.flush().unwrap();

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"world");
    });

    server_thread.join().unwrap();
    client_thread.join().unwrap();
}

#[test]
fn test_listener_incoming_iterator() {
    let _ = &*SETUP;

    let server_addr = format!("{}:8081", INTERFACE_IP1);
    let server_addr_clone = server_addr.clone();

    let server_thread = thread::spawn(move || {
        let listener = XdpTcpListener::bind(server_addr_clone).unwrap();
        let mut incoming = listener.incoming();

        // Accept the first connection
        if let Some(Ok(mut stream)) = incoming.next() {
            stream.write_all(b"first").unwrap();
            stream.flush().unwrap();
        }

        // Accept the second connection
        if let Some(Ok(mut stream)) = incoming.next() {
            stream.write_all(b"second").unwrap();
            stream.flush().unwrap();
        }
    });

    // Give the server a moment to start listening
    thread::sleep(std::time::Duration::from_secs(1));

    let client1_thread = thread::spawn({
        let server_addr = server_addr.clone();
        move || {
            let mut stream = XdpTcpStream::connect(server_addr).unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).unwrap();
            assert_eq!(buf, b"first");
        }
    });

    // Ensure clients connect sequentially
    client1_thread.join().unwrap();

    let client2_thread = thread::spawn(move || {
        let mut stream = XdpTcpStream::connect(server_addr).unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"second");
    });

    server_thread.join().unwrap();
    client2_thread.join().unwrap();
}

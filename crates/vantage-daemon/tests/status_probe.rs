#![cfg(unix)]

//! `StatusProbe` must keep "no daemon is running" (an answer) apart from "the
//! endpoint exists but the exchange failed" (a fault). These tests pin that
//! classification against a real socket, without any live daemon install.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use vantage_daemon::{DaemonRequest, StatusProbe, VERSION, probe_at, request_at, run_at};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vantage-daemon-probe-test-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn a_missing_socket_path_is_absent_rather_than_a_fault() {
    let socket = socket_path();
    match probe_at(&socket) {
        StatusProbe::Absent { detail } => {
            assert!(
                detail.contains(&socket.display().to_string()),
                "absence detail should name the endpoint: {detail}"
            );
        }
        other => panic!("expected Absent, got {other:?}"),
    }
}

#[test]
fn a_running_daemon_reports_its_version_protocol_and_pid() {
    let socket = socket_path();
    let (_events, receiver) = mpsc::channel();
    let server_socket = socket.clone();
    let server = thread::spawn(move || run_at(&server_socket, receiver));

    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match probe_at(&socket) {
            StatusProbe::Running(status) => break status,
            _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            other => panic!("daemon did not answer the probe: {other:?}"),
        }
    };
    assert_eq!(status.protocol_version, 1);
    assert_eq!(status.version, VERSION);
    assert_eq!(status.pid, std::process::id());
    assert_eq!(status.subscribers, 0);

    assert!(request_at(&socket, &DaemonRequest::Stop).unwrap().ok);
    server.join().unwrap().unwrap();
    assert!(!socket.exists());
}

#[test]
fn a_daemon_that_rejects_the_status_request_is_a_fault() {
    let socket = socket_path();
    let listener = UnixListener::bind(&socket).unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut request).unwrap();
        stream.write_all(b"{\"ok\":false,\"error\":\"probe rejected\"}\n").unwrap();
    });

    match probe_at(&socket) {
        StatusProbe::Fault { detail } => assert_eq!(detail, "probe rejected"),
        other => panic!("expected Fault, got {other:?}"),
    }

    server.join().unwrap();
    std::fs::remove_file(&socket).unwrap();
}

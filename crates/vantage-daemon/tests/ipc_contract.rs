#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use vantage_daemon::{DaemonEvent, DaemonRequest, DaemonStatus, request_at, run_at};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vantage-daemon-test-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn same_uid_client_observes_bounded_status_events_and_stops_server() {
    let socket = socket_path();
    let (events, receiver) = mpsc::channel();
    let server_socket = socket.clone();
    let server = thread::spawn(move || run_at(&server_socket, receiver));

    let deadline = Instant::now() + Duration::from_secs(2);
    let initial = loop {
        match request_at(&socket, &DaemonRequest::Status) {
            Ok(response) => break response,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("daemon did not become ready: {error}"),
        }
    };
    assert!(initial.ok);

    events
        .send(DaemonEvent::now(
            "test_event",
            serde_json::json!({"value": 42}),
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        let response = request_at(&socket, &DaemonRequest::Status).unwrap();
        let status: DaemonStatus = serde_json::from_value(response.data.unwrap()).unwrap();
        if status
            .last_events
            .iter()
            .any(|event| event.kind == "test_event")
        {
            break status;
        }
        assert!(Instant::now() < deadline, "event was not published");
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status.protocol_version, 1);
    assert_eq!(status.pid, std::process::id());

    let stopped = request_at(&socket, &DaemonRequest::Stop).unwrap();
    assert!(stopped.ok);
    server.join().unwrap().unwrap();
    assert!(!socket.exists());
}

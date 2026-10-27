#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use vantage_daemon::{
    DaemonEvent, DaemonRequest, DaemonResponse, DaemonStatus, MAX_EVENTS, request_at, run_at,
};

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "vantage-daemon-event-test-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

fn wait_ready(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match request_at(socket, &DaemonRequest::Status) {
            Ok(response) if response.ok => return,
            _ if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            result => panic!("daemon did not become ready: {result:?}"),
        }
    }
}

fn status(socket: &Path) -> DaemonStatus {
    let response = request_at(socket, &DaemonRequest::Status).unwrap();
    serde_json::from_value(response.data.unwrap()).unwrap()
}

#[test]
fn event_history_retains_only_the_newest_64_events() {
    let socket = socket_path();
    let (events, receiver) = mpsc::channel();
    let server_socket = socket.clone();
    let server = thread::spawn(move || run_at(&server_socket, receiver));
    wait_ready(&socket);

    for sequence in 0..65 {
        events
            .send(DaemonEvent::snapshot_changed(json!({"sequence": sequence})))
            .unwrap();
    }
    let status = status(&socket);
    assert_eq!(status.last_events.len(), MAX_EVENTS);
    assert_eq!(status.last_events.first().unwrap().payload["sequence"], json!(1));
    assert_eq!(status.last_events.last().unwrap().payload["sequence"], json!(64));

    assert!(request_at(&socket, &DaemonRequest::Stop).unwrap().ok);
    server.join().unwrap().unwrap();
}

#[test]
fn a_slow_subscriber_is_dropped_without_blocking_event_ingestion() {
    let socket = socket_path();
    let (events, receiver) = mpsc::channel();
    let server_socket = socket.clone();
    let server = thread::spawn(move || run_at(&server_socket, receiver));
    wait_ready(&socket);

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    let request = serde_json::to_vec(&DaemonRequest::Subscribe).unwrap();
    subscriber.write_all(&request).unwrap();
    subscriber.write_all(b"\n").unwrap();
    subscriber
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut subscriber = BufReader::new(subscriber);
    let mut line = String::new();
    subscriber.read_line(&mut line).unwrap();
    let acknowledgement: DaemonResponse = serde_json::from_str(line.trim_end()).unwrap();
    assert!(acknowledgement.ok);

    // Do not read the event stream. These events exceed socket buffering and
    // the daemon's eight-event per-subscriber queue, but remain below the IPC
    // message limit individually.
    for sequence in 0..24 {
        events
            .send(DaemonEvent::snapshot_changed(json!({
                "sequence": sequence,
                "blob": "x".repeat(48_000),
            })))
            .unwrap();
    }
    for sequence in 24..104 {
        events
            .send(DaemonEvent::snapshot_changed(json!({"sequence": sequence})))
            .unwrap();
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        // A subsequent broadcast also removes a writer that disconnected after
        // a write timeout, not just a sender whose queue was already full.
        events
            .send(DaemonEvent::snapshot_changed(json!({"sequence": "liveness"})))
            .unwrap();
        let current = status(&socket);
        assert_eq!(current.last_events.len(), MAX_EVENTS);
        if current.subscribers == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "slow subscriber was not dropped");
        thread::sleep(Duration::from_millis(20));
    }

    assert!(request_at(&socket, &DaemonRequest::Stop).unwrap().ok);
    server.join().unwrap().unwrap();
}

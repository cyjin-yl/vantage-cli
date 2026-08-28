use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use lctrl_core::{LctrlError, Result};

use crate::{
    DaemonEvent, DaemonRequest, DaemonResponse, DaemonStatus, MAX_EVENTS, PROTOCOL_VERSION,
    protocol_error, started_at_unix_ms,
};

pub fn default_endpoint() -> PathBuf {
    std::env::var_os("VANTAGE_SOCKET")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map(|root| PathBuf::from(root).join("vantage.sock"))
        })
        .unwrap_or_else(|| PathBuf::from("/run/vantage.sock"))
}

pub fn run(events: Receiver<DaemonEvent>) -> Result<()> {
    run_at(&default_endpoint(), events)
}

pub fn run_at(endpoint: &Path, events: Receiver<DaemonEvent>) -> Result<()> {
    if endpoint.exists() {
        if request_at(endpoint, &DaemonRequest::Status).is_ok() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("daemon already listening at {}", endpoint.display()),
            });
        }
        fs::remove_file(endpoint).map_err(LctrlError::Io)?;
    }
    if let Some(parent) = endpoint.parent() {
        fs::create_dir_all(parent).map_err(LctrlError::Io)?;
    }
    let listener = UnixListener::bind(endpoint).map_err(LctrlError::Io)?;
    fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600)).map_err(LctrlError::Io)?;
    listener.set_nonblocking(true).map_err(LctrlError::Io)?;
    let _cleanup = SocketCleanup(endpoint.to_path_buf());

    let started_at_unix_ms = started_at_unix_ms();
    let mut last_events = VecDeque::with_capacity(MAX_EVENTS);
    let mut subscribers = Vec::<SyncSender<DaemonEvent>>::new();
    let mut stop = false;
    while !stop {
        while let Ok(event) = events.try_recv() {
            if last_events.len() == MAX_EVENTS {
                last_events.pop_front();
            }
            last_events.push_back(event.clone());
            broadcast(&mut subscribers, event);
        }

        loop {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(LctrlError::Io(error)),
            };
            verify_peer(&stream)?;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .map_err(LctrlError::Io)?;
            let mut reader = BufReader::new(stream.try_clone().map_err(LctrlError::Io)?);
            let request_line = match read_limited_line(&mut reader) {
                Ok(Some(line)) => line,
                Ok(None) => continue,
                Err(error) => {
                    let _ = write_line(&mut stream, &DaemonResponse::failure(error.to_string()));
                    continue;
                }
            };
            let request = match serde_json::from_str::<DaemonRequest>(request_line.trim_end()) {
                Ok(request) => request,
                Err(error) => {
                    write_line(&mut stream, &DaemonResponse::failure(error.to_string()))?;
                    continue;
                }
            };
            let status = || DaemonStatus {
                protocol_version: PROTOCOL_VERSION,
                pid: std::process::id(),
                started_at_unix_ms,
                subscribers: subscribers.len(),
                last_events: last_events.iter().cloned().collect(),
            };
            match request {
                DaemonRequest::Status => {
                    write_line(&mut stream, &DaemonResponse::success(status()))?;
                }
                DaemonRequest::Stop => {
                    write_line(&mut stream, &DaemonResponse::success(status()))?;
                    stop = true;
                }
                DaemonRequest::Subscribe => {
                    stream
                        .set_write_timeout(Some(Duration::from_millis(100)))
                        .map_err(LctrlError::Io)?;
                    write_line(&mut stream, &DaemonResponse::success(status()))?;
                    let (sender, receiver) = sync_channel(8);
                    subscribers.push(sender);
                    thread::spawn(move || {
                        while let Ok(event) = receiver.recv() {
                            if write_line(&mut stream, &event).is_err() {
                                break;
                            }
                        }
                    });
                }
            }
        }
        if !stop {
            thread::sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}

pub fn request(request: &DaemonRequest) -> Result<DaemonResponse> {
    request_at(&default_endpoint(), request)
}

pub fn request_at(endpoint: &Path, request: &DaemonRequest) -> Result<DaemonResponse> {
    let mut stream = UnixStream::connect(endpoint)
        .map_err(|error| protocol_error(format!("connect {}: {error}", endpoint.display())))?;
    verify_peer(&stream)?;
    write_line(&mut stream, request)?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(LctrlError::Io)?;
    if response.is_empty() {
        return Err(protocol_error("server closed without a response"));
    }
    serde_json::from_str(response.trim_end())
        .map_err(|error| protocol_error(format!("invalid response: {error}")))
}

const MAX_MESSAGE_BYTES: usize = 64 * 1024;

fn read_limited_line(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().map_err(LctrlError::Io)?;
        if chunk.is_empty() {
            break;
        }
        let (take, complete) = match chunk.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (chunk.len(), false),
        };
        if bytes.len().saturating_add(take) > MAX_MESSAGE_BYTES {
            return Err(protocol_error(format!(
                "request exceeds {MAX_MESSAGE_BYTES} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk[..take]);
        reader.consume(take);
        if complete {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| protocol_error(format!("request is not UTF-8: {error}")))
}

fn broadcast(subscribers: &mut Vec<SyncSender<DaemonEvent>>, event: DaemonEvent) {
    subscribers.retain(|subscriber| match subscriber.try_send(event.clone()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    });
}

fn write_line<T: serde::Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
    bytes.push(b'\n');
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(protocol_error(format!(
            "response exceeds {MAX_MESSAGE_BYTES} bytes"
        )));
    }
    stream.write_all(&bytes).map_err(LctrlError::Io)?;
    stream.flush().map_err(LctrlError::Io)
}
fn verify_peer(stream: &UnixStream) -> Result<()> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if status != 0 {
        return Err(LctrlError::Io(std::io::Error::last_os_error()));
    }
    let current_uid = unsafe { libc::geteuid() };
    if credentials.uid != current_uid {
        return Err(LctrlError::PermissionDenied {
            need: format!(
                "daemon peer uid {} must match current uid {current_uid}",
                credentials.uid
            ),
        });
    }
    Ok(())
}

struct SocketCleanup(PathBuf);

impl Drop for SocketCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

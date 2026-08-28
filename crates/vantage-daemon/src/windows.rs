use std::collections::VecDeque;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

use lctrl_core::{LctrlError, Result};
use parking_lot::Mutex;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    },
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        WaitNamedPipeW,
    },
};

use crate::{
    DaemonEvent, DaemonRequest, DaemonResponse, DaemonStatus, MAX_EVENTS, PROTOCOL_VERSION,
    protocol_error, started_at_unix_ms,
};

const BUFFER_SIZE: u32 = 64 * 1024;

pub fn default_endpoint() -> String {
    std::env::var("VANTAGE_PIPE").unwrap_or_else(|_| r"\\.\pipe\vantage.sock".into())
}

pub fn run(events: Receiver<DaemonEvent>) -> Result<()> {
    let endpoint = wide(&default_endpoint());
    let descriptor = SecurityDescriptor::new()?;
    let stop = Arc::new(AtomicBool::new(false));
    let clients = Arc::new(AtomicUsize::new(0));
    let last_events = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_EVENTS)));
    let subscribers = Arc::new(Mutex::new(Vec::<SyncSender<DaemonEvent>>::new()));
    let event_thread = {
        let stop = Arc::clone(&stop);
        let last_events = Arc::clone(&last_events);
        let subscribers = Arc::clone(&subscribers);
        thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                match events.recv_timeout(Duration::from_millis(100)) {
                    Ok(event) => {
                        let mut history = last_events.lock();
                        if history.len() == MAX_EVENTS {
                            history.pop_front();
                        }
                        history.push_back(event.clone());
                        drop(history);
                        broadcast(&subscribers, event);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        })
    };
    let started_at_unix_ms = started_at_unix_ms();

    while !stop.load(Ordering::Acquire) {
        let handle = unsafe {
            CreateNamedPipeW(
                endpoint.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER_SIZE,
                BUFFER_SIZE,
                0,
                &descriptor.attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            stop.store(true, Ordering::Release);
            let _ = event_thread.join();
            return Err(LctrlError::Io(std::io::Error::last_os_error()));
        }
        let connected = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) };
        if connected == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error().map(|code| code as u32) != Some(ERROR_PIPE_CONNECTED) {
                unsafe { CloseHandle(handle) };
                stop.store(true, Ordering::Release);
                let _ = event_thread.join();
                return Err(LctrlError::Io(error));
            }
        }
        if clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
            clients.fetch_sub(1, Ordering::AcqRel);
            let _ = write_value(handle, &DaemonResponse::failure("too many daemon clients"));
            close_pipe(handle);
            continue;
        }
        let stop_for_client = Arc::clone(&stop);
        let history_for_client = Arc::clone(&last_events);
        let subscribers_for_client = Arc::clone(&subscribers);
        let endpoint_for_client = endpoint.clone();
        let clients_for_client = Arc::clone(&clients);
        let pipe = PipeHandle(handle);
        thread::spawn(move || {
            handle_client(
                pipe,
                stop_for_client,
                history_for_client,
                subscribers_for_client,
                started_at_unix_ms,
                endpoint_for_client,
            );
            clients_for_client.fetch_sub(1, Ordering::AcqRel);
        });
    }

    stop.store(true, Ordering::Release);
    let _ = event_thread.join();
    subscribers.lock().clear();
    Ok(())
}

const MAX_CLIENTS: usize = 32;
const MAX_SUBSCRIBERS: usize = 16;

fn handle_client(
    pipe: PipeHandle,
    stop: Arc<AtomicBool>,
    last_events: Arc<Mutex<VecDeque<DaemonEvent>>>,
    subscribers: Arc<Mutex<Vec<SyncSender<DaemonEvent>>>>,
    started_at_unix_ms: u64,
    endpoint: Vec<u16>,
) {
    let handle = pipe.raw();
    let request = match read_request(handle) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_value(handle, &DaemonResponse::failure(error.to_string()));
            drop(pipe);
            return;
        }
    };
    let status = || DaemonStatus {
        protocol_version: PROTOCOL_VERSION,
        pid: std::process::id(),
        started_at_unix_ms,
        subscribers: subscribers.lock().len(),
        last_events: last_events.lock().iter().cloned().collect(),
    };
    match request {
        DaemonRequest::Status => {
            let _ = write_value(handle, &DaemonResponse::success(status()));
            drop(pipe);
        }
        DaemonRequest::Stop => {
            let _ = write_value(handle, &DaemonResponse::success(status()));
            drop(pipe);
            stop.store(true, Ordering::Release);
            wake_listener(&endpoint);
        }
        DaemonRequest::Subscribe => {
            if subscribers.lock().len() >= MAX_SUBSCRIBERS {
                let _ = write_value(
                    handle,
                    &DaemonResponse::failure("too many daemon subscribers"),
                );
                drop(pipe);
                return;
            }
            if write_value(handle, &DaemonResponse::success(status())).is_err() {
                drop(pipe);
                return;
            }
            let (sender, receiver) = sync_channel(8);
            subscribers.lock().push(sender);
            thread::spawn(move || {
                while let Ok(event) = receiver.recv() {
                    if write_value(pipe.raw(), &event).is_err() {
                        break;
                    }
                }
                drop(pipe);
            });
        }
    }
}

fn wake_listener(endpoint: &[u16]) {
    if unsafe { WaitNamedPipeW(endpoint.as_ptr(), 100) } == 0 {
        return;
    }
    let handle = unsafe {
        CreateFileW(
            endpoint.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle != INVALID_HANDLE_VALUE {
        unsafe { CloseHandle(handle) };
    }
}

pub fn request(request: &DaemonRequest) -> Result<DaemonResponse> {
    let endpoint = wide(&default_endpoint());
    let available = unsafe { WaitNamedPipeW(endpoint.as_ptr(), 2_000) };
    if available == 0 {
        return Err(protocol_error(std::io::Error::last_os_error().to_string()));
    }
    let handle = unsafe {
        CreateFileW(
            endpoint.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(protocol_error(std::io::Error::last_os_error().to_string()));
    }
    let result = (|| {
        write_value(handle, request)?;
        read_response(handle)
    })();
    unsafe { CloseHandle(handle) };
    result
}

fn read_request(handle: HANDLE) -> Result<DaemonRequest> {
    let bytes = read_message(handle)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| protocol_error(format!("invalid request: {error}")))
}

fn read_response(handle: HANDLE) -> Result<DaemonResponse> {
    let bytes = read_message(handle)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| protocol_error(format!("invalid response: {error}")))
}

fn read_message(handle: HANDLE) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; BUFFER_SIZE as usize];
    let mut read = 0u32;
    let success = unsafe {
        ReadFile(
            handle,
            buffer.as_mut_ptr(),
            BUFFER_SIZE,
            &mut read,
            ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(LctrlError::Io(std::io::Error::last_os_error()));
    }
    buffer.truncate(read as usize);
    Ok(buffer)
}

fn write_value<T: serde::Serialize>(handle: HANDLE, value: &T) -> Result<()> {
    let mut bytes =
        serde_json::to_vec(value).map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
    bytes.push(b'\n');
    if bytes.len() > BUFFER_SIZE as usize {
        return Err(LctrlError::InvalidArgument {
            detail: format!("daemon IPC message exceeds {BUFFER_SIZE} bytes"),
        });
    }
    let length = u32::try_from(bytes.len()).map_err(|_| LctrlError::InvalidArgument {
        detail: "daemon IPC message exceeds u32 length".into(),
    })?;
    let mut written = 0u32;
    let success = unsafe {
        WriteFile(
            handle,
            bytes.as_ptr(),
            length,
            &mut written,
            ptr::null_mut(),
        )
    };
    if success == 0 || written != length {
        return Err(LctrlError::Io(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn broadcast(subscribers: &Mutex<Vec<SyncSender<DaemonEvent>>>, event: DaemonEvent) {
    let mut subscribers = subscribers.lock();
    subscribers.retain(|subscriber| match subscriber.try_send(event.clone()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
    });
}

fn close_pipe(handle: HANDLE) {
    unsafe {
        DisconnectNamedPipe(handle);
        CloseHandle(handle);
    }
}

struct PipeHandle(HANDLE);
unsafe impl Send for PipeHandle {}

impl PipeHandle {
    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for PipeHandle {
    fn drop(&mut self) {
        close_pipe(self.0);
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

struct SecurityDescriptor {
    pointer: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

impl SecurityDescriptor {
    fn new() -> Result<Self> {
        let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)");
        let mut pointer: PSECURITY_DESCRIPTOR = ptr::null_mut();
        let success = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut pointer,
                ptr::null_mut(),
            )
        };
        if success == 0 {
            return Err(LctrlError::Io(std::io::Error::last_os_error()));
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: pointer,
            bInheritHandle: 0,
        };
        Ok(Self {
            pointer,
            attributes,
        })
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.pointer);
        }
    }
}

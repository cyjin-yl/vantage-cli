//! Local authenticated IPC and bounded event buffering for the optional vantage daemon.

use std::time::{SystemTime, UNIX_EPOCH};

use lctrl_core::LctrlError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_EVENTS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DaemonRequest {
    Status,
    Stop,
    Subscribe,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl DaemonResponse {
    fn success<T: Serialize>(value: T) -> Self {
        match serde_json::to_value(value) {
            Ok(data) => Self {
                ok: true,
                data: Some(data),
                error: None,
            },
            Err(error) => Self::failure(error.to_string()),
        }
    }

    fn failure(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonEvent {
    pub ts_unix_ms: u64,
    pub kind: String,
    pub payload: Value,
}

impl DaemonEvent {
    #[must_use]
    pub fn now(kind: impl Into<String>, payload: Value) -> Self {
        let ts_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            });
        Self {
            ts_unix_ms,
            kind: kind.into(),
            payload,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonStatus {
    pub protocol_version: u32,
    pub pid: u32,
    pub started_at_unix_ms: u64,
    pub subscribers: usize,
    pub last_events: Vec<DaemonEvent>,
}

fn started_at_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn protocol_error(error: impl Into<String>) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("vantage daemon IPC: {}", error.into()),
    }
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{default_endpoint, request, request_at, run, run_at};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{default_endpoint, request, run};

#[cfg(not(any(unix, windows)))]
pub fn run(_events: std::sync::mpsc::Receiver<DaemonEvent>) -> lctrl_core::Result<()> {
    Err(LctrlError::Unsupported {
        feature: "daemon.ipc".into(),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn request(_request: &DaemonRequest) -> lctrl_core::Result<DaemonResponse> {
    Err(LctrlError::Unsupported {
        feature: "daemon.ipc".into(),
    })
}

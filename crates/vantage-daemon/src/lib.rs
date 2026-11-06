//! Local authenticated IPC and bounded event buffering for the optional vantage daemon.

use std::time::{SystemTime, UNIX_EPOCH};

use lctrl_core::LctrlError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_EVENTS: usize = 64;
pub const VENDOR_EVENT_KIND: &str = "vendor_event";
pub const SNAPSHOT_CHANGED_KIND: &str = "snapshot_changed";
/// Crate version of this daemon build, reported as [`DaemonStatus::version`].
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

    #[must_use]
    pub fn snapshot_changed(payload: Value) -> Self {
        Self::now(SNAPSHOT_CHANGED_KIND, payload)
    }

    #[cfg(windows)]
    pub fn vendor_wmi(event: lctrl_hal::RawEvent) -> lctrl_core::Result<Self> {
        #[derive(Serialize)]
        struct VendorWmiPayload {
            source_class: String,
            instance: std::collections::BTreeMap<String, lctrl_hal::EventValue>,
        }

        let lctrl_hal::RawEvent {
            timestamp_unix_ms,
            source_class,
            instance,
        } = event;
        let payload = serde_json::to_value(VendorWmiPayload {
            source_class,
            instance,
        })
        .map_err(|error| LctrlError::Io(std::io::Error::other(error)))?;
        Ok(Self {
            ts_unix_ms: timestamp_unix_ms,
            kind: VENDOR_EVENT_KIND.into(),
            payload,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonStatus {
    pub protocol_version: u32,
    /// Crate version of the daemon that answered, so a reply identifies the
    /// build behind it. Daemons older than this field omit it.
    #[serde(default)]
    pub version: String,
    pub pid: u32,
    pub started_at_unix_ms: u64,
    pub subscribers: usize,
    pub last_events: Vec<DaemonEvent>,
}

/// Presence of the daemon at an IPC endpoint, as observed by one status probe.
///
/// The clients use this instead of a bare [`LctrlError`] so that "no daemon is
/// running" (an answer) stays distinct from "the endpoint is broken"
/// (a fault): only the latter is an error to report.
#[derive(Clone, Debug)]
pub enum StatusProbe {
    /// The daemon accepted the request and answered `Status`.
    Running(DaemonStatus),
    /// Nothing is listening: the endpoint does not exist or refused the connection.
    Absent { detail: String },
    /// The endpoint exists but the exchange failed, so the daemon state is unknown.
    Fault { detail: String },
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
pub use unix::{default_endpoint, endpoint_text, probe, probe_at, request, request_at, run, run_at};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{default_endpoint, endpoint_text, probe, request, run};

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

#[cfg(not(any(unix, windows)))]
pub fn probe() -> StatusProbe {
    StatusProbe::Fault {
        detail: "daemon IPC is unsupported on this platform".into(),
    }
}

#[cfg(not(any(unix, windows)))]
pub fn probe_at(_endpoint: &std::path::Path) -> StatusProbe {
    probe()
}

#[cfg(not(any(unix, windows)))]
pub fn endpoint_text() -> String {
    "unsupported".into()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DaemonEvent, SNAPSHOT_CHANGED_KIND, VENDOR_EVENT_KIND};

    #[test]
    fn polling_snapshots_are_not_vendor_events() {
        let event = DaemonEvent::snapshot_changed(json!({"adapter": {"online": true}}));
        assert_eq!(event.kind, SNAPSHOT_CHANGED_KIND);
        assert_ne!(event.kind, VENDOR_EVENT_KIND);
        assert_eq!(event.payload["adapter"]["online"], json!(true));
    }

    #[cfg(windows)]
    #[test]
    fn vendor_events_expose_only_timestamp_class_and_actual_instance() {
        use std::collections::BTreeMap;

        use lctrl_hal::{EventValue, RawEvent};

        let raw = RawEvent::new(
            42,
            "LENOVO_LIGHTING_EVENT",
            BTreeMap::from([
                ("Key_ID".into(), EventValue::U8(7)),
                ("TIME_CREATED".into(), EventValue::U64(99)),
            ]),
        )
        .unwrap();
        let event = DaemonEvent::vendor_wmi(raw).unwrap();
        assert_eq!(event.ts_unix_ms, 42);
        assert_eq!(event.kind, VENDOR_EVENT_KIND);
        assert_eq!(
            event.payload,
            json!({
                "source_class": "LENOVO_LIGHTING_EVENT",
                "instance": {"Key_ID": 7, "TIME_CREATED": 99}
            })
        );
    }
}

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lctrl_core::{LctrlError, Result};
use serde::Serialize;

/// A provider value with its numeric width retained in Rust.
///
/// Numeric widths are retained in Rust so consumers do not have to infer a
/// vendor field's type from its value. JSON serialization uses the natural
/// scalar or array representation; Empty and Null both serialize as null.
#[derive(Clone, Debug, PartialEq)]
pub enum EventValue {
    Empty,
    Null,
    String(String),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Array(Vec<Self>),
}

impl Serialize for EventValue {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Empty | Self::Null => serializer.serialize_none(),
            Self::String(value) => serializer.serialize_str(value),
            Self::I8(value) => serializer.serialize_i8(*value),
            Self::I16(value) => serializer.serialize_i16(*value),
            Self::I32(value) => serializer.serialize_i32(*value),
            Self::I64(value) => serializer.serialize_i64(*value),
            Self::U8(value) => serializer.serialize_u8(*value),
            Self::U16(value) => serializer.serialize_u16(*value),
            Self::U32(value) => serializer.serialize_u32(*value),
            Self::U64(value) => serializer.serialize_u64(*value),
            Self::F32(value) => serializer.serialize_f32(*value),
            Self::F64(value) => serializer.serialize_f64(*value),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Array(values) => values.serialize(serializer),
        }
    }
}

/// An event delivered by a vendor provider, without semantic interpretation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RawEvent {
    /// Local receipt time. Provider TIME_CREATED remains in `instance` when present.
    pub timestamp_unix_ms: u64,
    pub source_class: String,
    pub instance: BTreeMap<String, EventValue>,
}

impl RawEvent {
    pub fn new(
        timestamp_unix_ms: u64,
        source_class: impl Into<String>,
        instance: BTreeMap<String, EventValue>,
    ) -> Result<Self> {
        let source_class = source_class.into();
        if source_class.is_empty()
            || !source_class
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(LctrlError::InvalidArgument {
                detail: format!("invalid event source class: {source_class:?}"),
            });
        }
        Ok(Self {
            timestamp_unix_ms,
            source_class,
            instance,
        })
    }

    #[must_use]
    pub fn time_created(&self) -> Option<u64> {
        match self.instance.get("TIME_CREATED") {
            Some(EventValue::U64(value)) => Some(*value),
            _ => None,
        }
    }
}

/// Cooperative cancellation shared with a blocking raw-event reader.
#[derive(Clone, Debug, Default)]
pub struct EventCancellation(Arc<AtomicBool>);

impl EventCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// A blocking reader. `Ok(None)` means cancellation or source termination.
pub trait RawEventReader {
    fn read(&mut self) -> Result<Option<RawEvent>>;
}

/// Event sources may borrow native COM state, so readers are intentionally not Send.
pub trait EventSource {
    fn subscribe<'a>(
        &'a self,
        source_classes: &[&str],
        cancellation: EventCancellation,
    ) -> Result<Box<dyn RawEventReader + 'a>>;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{EventCancellation, RawEvent};

    #[test]
    fn cancellation_is_shared_by_all_clones() {
        let cancellation = EventCancellation::new();
        let clone = cancellation.clone();
        assert!(!cancellation.is_cancelled());
        clone.cancel();
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn raw_event_requires_a_real_source_class() {
        assert!(RawEvent::new(1, "", BTreeMap::new()).is_err());
        assert!(RawEvent::new(1, "not a WMI class", BTreeMap::new()).is_err());
        assert!(RawEvent::new(1, "LENOVO_AC_PD_EVENT", BTreeMap::new()).is_ok());
    }
}

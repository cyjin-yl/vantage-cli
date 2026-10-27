#[cfg(any(windows, test))]
use lctrl_core::Result;
#[cfg(any(windows, test))]
use lctrl_hal::{EventValue, RawEvent};

#[cfg(any(windows, test))]
use crate::{WmiObject, WmiValue};

/// All provider event classes documented for the target platform.
///
/// Subscription is per class: firmware omissions are skipped, never replaced
/// with synthetic state-change or key-action events.
pub const DOCUMENTED_LENOVO_EVENT_CLASSES: &[&str] = &[
    "LENOVO_AC_PD_EVENT",
    "LENOVO_AI_CHIP_EVENT",
    "LENOVO_AI_SCENARIO_TYPE_EVENT",
    "LENOVO_BTKBD_EVENT",
    "LENOVO_DISPATCHER_EVENT",
    "LENOVO_GAMEZONE_FAN_COOLING_EVENT",
    "LENOVO_GAMEZONE_KEYLOCK_STATUS_EVENT",
    "LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT",
    "LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT",
    "LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT",
    "LENOVO_GAMEZONE_SMART_FAN_SETTING_EVENT",
    "LENOVO_GAMEZONE_THERMAL_MODE_EVENT",
    "LENOVO_LIGHTING_EVENT",
    "LENOVO_REPORT_2D3D_STATUS_EVENT",
    "LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT",
    "LENOVO_REPORT_REFRESH_RATE_EVENT",
    "LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT",
    "LENOVO_SMART_THERMAL_MONITOR_EVENT",
    "LENOVO_SR_EVENT",
    "LENOVO_UTILITY_EVENT",
];

#[cfg(any(windows, test))]
fn raw_event_from_object(
    timestamp_unix_ms: u64,
    source_class: &str,
    object: WmiObject,
) -> Result<RawEvent> {
    let instance = object
        .into_iter()
        .map(|(name, value)| (name, event_value(value)))
        .collect();
    RawEvent::new(timestamp_unix_ms, source_class, instance)
}

#[cfg(any(windows, test))]
fn event_value(value: WmiValue) -> EventValue {
    match value {
        WmiValue::Empty => EventValue::Empty,
        WmiValue::Null => EventValue::Null,
        WmiValue::String(value) => EventValue::String(value),
        WmiValue::I8(value) => EventValue::I8(value),
        WmiValue::I16(value) => EventValue::I16(value),
        WmiValue::I32(value) => EventValue::I32(value),
        WmiValue::I64(value) => EventValue::I64(value),
        WmiValue::U8(value) => EventValue::U8(value),
        WmiValue::U16(value) => EventValue::U16(value),
        WmiValue::U32(value) => EventValue::U32(value),
        WmiValue::U64(value) => EventValue::U64(value),
        WmiValue::F32(value) => EventValue::F32(value),
        WmiValue::F64(value) => EventValue::F64(value),
        WmiValue::Bool(value) => EventValue::Bool(value),
        WmiValue::Array(values) => {
            EventValue::Array(values.into_iter().map(event_value).collect())
        }
    }
}

#[cfg(windows)]
mod native {
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use futures::{Stream, StreamExt};
    use lctrl_core::{LctrlError, Result};
    use lctrl_hal::{EventCancellation, EventSource, RawEvent, RawEventReader};
    use wmi::{IWbemClassWrapper, Variant, WMIConnection, WMIError, WMIResult};

    use super::raw_event_from_object;
    use crate::error::map_wmi_hresult;
    use crate::wmi_contract::ROOT_WMI;
    use crate::{WmiObject, WmiValue};

    const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);

    type RawResultStream<'a> = Pin<Box<dyn Stream<Item = Result<RawEvent>> + 'a>>;

    /// A COM-bound source. Construct and read it on the same thread.
    pub struct NativeWmiEventSource {
        connection: WMIConnection,
    }

    impl NativeWmiEventSource {
        pub fn new() -> Result<Self> {
            let connection = WMIConnection::with_namespace_path(ROOT_WMI)
                .map_err(|error| map_notification_error(error, "connect ROOT\\WMI events"))?;
            Ok(Self { connection })
        }

        pub fn subscribe_native<'a>(
            &'a self,
            source_classes: &[&str],
            cancellation: EventCancellation,
        ) -> Result<NativeWmiEventReader<'a>> {
            subscribe_with(source_classes, cancellation, |source_class| {
                let query = format!("SELECT * FROM {source_class}");
                let stream = match self.connection.exec_notification_query_async(query) {
                    Ok(stream) => stream,
                    Err(error) => {
                        let error = map_notification_error(
                            error,
                            &format!("subscribe {ROOT_WMI} {source_class}"),
                        );
                        if matches!(error, LctrlError::Unsupported { .. }) {
                            return Ok(None);
                        }
                        return Err(error);
                    }
                };
                let subscribed_class = String::from(source_class);
                let stream: RawResultStream<'a> = Box::pin(stream.map(move |result| {
                    notification_to_raw(result, &subscribed_class)
                }));
                Ok(Some(stream))
            })
        }
    }

    impl EventSource for NativeWmiEventSource {
        fn subscribe<'a>(
            &'a self,
            source_classes: &[&str],
            cancellation: EventCancellation,
        ) -> Result<Box<dyn RawEventReader + 'a>> {
            Ok(Box::new(
                self.subscribe_native(source_classes, cancellation)?,
            ))
        }
    }

    pub struct NativeWmiEventReader<'a> {
        streams: Vec<RawResultStream<'a>>,
        cancellation: EventCancellation,
        waker: Waker,
        next: usize,
    }

    impl<'a> NativeWmiEventReader<'a> {
        fn new(streams: Vec<RawResultStream<'a>>, cancellation: EventCancellation) -> Self {
            let waker = Waker::from(Arc::new(UnparkThread(thread::current())));
            Self {
                streams,
                cancellation,
                waker,
                next: 0,
            }
        }
    }

    impl RawEventReader for NativeWmiEventReader<'_> {
        fn read(&mut self) -> Result<Option<RawEvent>> {
            let waker = self.waker.clone();
            let mut context = Context::from_waker(&waker);
            loop {
                if self.cancellation.is_cancelled() {
                    // Dropping wmi's async streams invokes CancelAsyncCall even
                    // if the caller retains this reader after cancellation.
                    self.streams.clear();
                    return Ok(None);
                }
                if self.streams.is_empty() {
                    return Ok(None);
                }
                let count = self.streams.len();
                let mut pending = false;
                let mut removed = false;
                for _ in 0..count {
                    let index = self.next;
                    match self.streams[index].as_mut().poll_next(&mut context) {
                        Poll::Ready(Some(Ok(event))) => {
                            self.next = (index + 1) % self.streams.len();
                            return Ok(Some(event));
                        }
                        Poll::Ready(Some(Err(error))) => return Err(error),
                        Poll::Ready(None) => {
                            drop(self.streams.remove(index));
                            if self.streams.is_empty() {
                                return Ok(None);
                            }
                            self.next = index % self.streams.len();
                            removed = true;
                            break;
                        }
                        Poll::Pending => {
                            pending = true;
                            self.next = (index + 1) % count;
                        }
                    }
                }
                if removed {
                    continue;
                }
                if pending {
                    thread::park_timeout(CANCELLATION_POLL_INTERVAL);
                }
            }
        }
    }

    fn subscribe_with<'a, F>(
        source_classes: &[&str],
        cancellation: EventCancellation,
        mut open: F,
    ) -> Result<NativeWmiEventReader<'a>>
    where
        F: FnMut(&str) -> Result<Option<RawResultStream<'a>>>,
    {
        if source_classes.is_empty() {
            return Err(LctrlError::InvalidArgument {
                detail: "at least one WMI event class is required".into(),
            });
        }
        let mut streams = Vec::new();
        for source_class in source_classes {
            if cancellation.is_cancelled() {
                break;
            }
            validate_query_class(source_class)?;
            if let Some(stream) = open(source_class)? {
                streams.push(stream);
            }
        }
        if streams.is_empty() && !cancellation.is_cancelled() {
            return Err(LctrlError::Unsupported {
                feature: "wmi.events.no_requested_classes_available".into(),
            });
        }
        Ok(NativeWmiEventReader::new(streams, cancellation))
    }

    fn validate_query_class(source_class: &str) -> Result<()> {
        if !source_class.is_empty()
            && source_class
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Ok(());
        }
        Err(LctrlError::InvalidArgument {
            detail: format!("invalid WMI event class: {source_class:?}"),
        })
    }

    fn notification_to_raw(
        result: WMIResult<IWbemClassWrapper>,
        subscribed_class: &str,
    ) -> Result<RawEvent> {
        let wrapper = result.map_err(|error| {
            map_notification_error(error, &format!("read {subscribed_class} notification"))
        })?;
        let source_class = wrapper.class().map_err(|error| {
            map_notification_error(error, &format!("read {subscribed_class} __CLASS"))
        })?;
        let properties = wrapper.list_properties().map_err(|error| {
            map_notification_error(error, &format!("list {source_class} properties"))
        })?;
        let mut object = WmiObject::new();
        for name in properties {
            let value = wrapper.get_property(&name).map_err(|error| {
                map_notification_error(error, &format!("read {source_class}.{name}"))
            })?;
            object.insert(name, from_variant(value)?);
        }
        raw_event_from_object(received_at_unix_ms(), &source_class, object)
    }

    fn received_at_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            })
    }

    fn from_variant(value: Variant) -> Result<WmiValue> {
        Ok(match value {
            Variant::Empty => WmiValue::Empty,
            Variant::Null => WmiValue::Null,
            Variant::String(value) => WmiValue::String(value),
            Variant::I1(value) => WmiValue::I8(value),
            Variant::I2(value) => WmiValue::I16(value),
            Variant::I4(value) => WmiValue::I32(value),
            Variant::I8(value) => WmiValue::I64(value),
            Variant::R4(value) => WmiValue::F32(value),
            Variant::R8(value) => WmiValue::F64(value),
            Variant::Bool(value) => WmiValue::Bool(value),
            Variant::UI1(value) => WmiValue::U8(value),
            Variant::UI2(value) => WmiValue::U16(value),
            Variant::UI4(value) => WmiValue::U32(value),
            Variant::UI8(value) => WmiValue::U64(value),
            Variant::Array(values) => WmiValue::Array(
                values
                    .into_iter()
                    .map(from_variant)
                    .collect::<Result<Vec<_>>>()?,
            ),
            Variant::Unknown(_) | Variant::Object(_) => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WMI event contained an unsupported object/unknown VARIANT".into(),
                });
            }
        })
    }

    fn map_notification_error(error: WMIError, context: &str) -> LctrlError {
        match error {
            WMIError::HResultError { hres } => map_wmi_hresult(hres, context),
            other => LctrlError::Io(io::Error::other(format!("{context}: {other}"))),
        }
    }

    struct UnparkThread(thread::Thread);

    impl Wake for UnparkThread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    #[cfg(test)]
    mod tests {
        use std::thread;
        use std::time::{Duration, Instant};

        use futures::stream;
        use lctrl_core::{LctrlError, Result};
        use lctrl_hal::{EventCancellation, RawEvent, RawEventReader};

        use super::{NativeWmiEventReader, RawResultStream, subscribe_with};

        #[test]
        fn absent_classes_produce_no_fabricated_reader_or_event() {
            let result = subscribe_with(
                &["LENOVO_ABSENT_EVENT"],
                EventCancellation::new(),
                |_| -> Result<Option<RawResultStream<'static>>> { Ok(None) },
            );
            assert!(matches!(result, Err(LctrlError::Unsupported { .. })));
        }

        #[test]
        fn terminated_stream_does_not_fabricate_an_event() {
            let stream: RawResultStream<'static> =
                Box::pin(stream::empty::<Result<RawEvent>>());
            let mut reader =
                NativeWmiEventReader::new(vec![stream], EventCancellation::new());
            assert!(reader.read().unwrap().is_none());
        }

        #[test]
        fn cancellation_interrupts_a_pending_read() {
            let cancellation = EventCancellation::new();
            let cancel_from_thread = cancellation.clone();
            let stream: RawResultStream<'static> =
                Box::pin(stream::pending::<Result<RawEvent>>());
            let mut reader = NativeWmiEventReader::new(vec![stream], cancellation);
            let canceller = thread::spawn(move || {
                thread::sleep(Duration::from_millis(10));
                cancel_from_thread.cancel();
            });
            let started = Instant::now();
            assert!(reader.read().unwrap().is_none());
            canceller.join().unwrap();
            assert!(started.elapsed() < Duration::from_secs(1));
        }
    }
}

#[cfg(windows)]
pub use native::{NativeWmiEventReader, NativeWmiEventSource};

#[cfg(test)]
mod tests {
    use lctrl_hal::EventValue;
    use serde_json::{Value, json};

    use super::raw_event_from_object;
    use crate::{WmiObject, WmiValue};

    fn assert_class_schema(
        source_class: &str,
        property: &str,
        value: WmiValue,
        expected: Value,
    ) {
        let object = WmiObject::from([
            (
                "SECURITY_DESCRIPTOR".into(),
                WmiValue::Array(vec![WmiValue::U8(1), WmiValue::U8(2)]),
            ),
            ("TIME_CREATED".into(), WmiValue::U64(133_900_000_000_000_000)),
            ("Active".into(), WmiValue::Bool(true)),
            ("InstanceName".into(), WmiValue::String("ACPI\\VPC2004\\0".into())),
            (property.into(), value.clone()),
        ]);
        let event = raw_event_from_object(1_234, source_class, object).unwrap();
        assert_eq!(event.timestamp_unix_ms, 1_234);
        assert_eq!(event.source_class, source_class);
        assert_eq!(event.time_created(), Some(133_900_000_000_000_000));
        let serialized = serde_json::to_value(&event).unwrap();
        assert_eq!(serialized["timestamp_unix_ms"], json!(1_234));
        assert_eq!(serialized["source_class"], json!(source_class));
        assert_eq!(serialized["instance"][property], expected);
        assert_eq!(serialized["instance"]["TIME_CREATED"], json!(133_900_000_000_000_000_u64));
        assert_eq!(serialized["instance"]["SECURITY_DESCRIPTOR"], json!([1, 2]));
        assert!(serialized["instance"].get("action").is_none());
        assert!(serialized["instance"].get("key_action").is_none());
    }

    #[test]
    fn ac_dispatcher_lighting_and_refresh_classes_keep_their_raw_schema() {
        assert_class_schema(
            "LENOVO_AC_PD_EVENT",
            "AC_PD_Status",
            WmiValue::U16(2),
            json!(2),
        );
        assert_class_schema(
            "LENOVO_DISPATCHER_EVENT",
            "PowerLevel",
            WmiValue::U32(3),
            json!(3),
        );
        assert_class_schema(
            "LENOVO_LIGHTING_EVENT",
            "Key_ID",
            WmiValue::U8(1),
            json!(1),
        );
        assert_class_schema(
            "LENOVO_REPORT_REFRESH_RATE_EVENT",
            "MaxRefreshRate",
            WmiValue::U16(120),
            json!(120),
        );
    }

    #[test]
    fn refresh_rate_instance_preserves_both_actual_fields() {
        let object = WmiObject::from([
            ("MaxRefreshRate".into(), WmiValue::U16(120)),
            ("MinRefreshRate".into(), WmiValue::U16(60)),
        ]);
        let event = raw_event_from_object(
            99,
            "LENOVO_REPORT_REFRESH_RATE_EVENT",
            object,
        )
        .unwrap();
        assert_eq!(event.instance.get("MinRefreshRate"), Some(&EventValue::U16(60)));
        assert_eq!(event.instance.get("MaxRefreshRate"), Some(&EventValue::U16(120)));
        assert_eq!(event.instance.len(), 2);
    }
}

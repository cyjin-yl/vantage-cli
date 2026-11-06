//! Read-only Lenovo records from the operating system event log.
//!
//! The contract below was recovered from the live 21VG machine; the raw
//! observation is `docs/evidence/re-eventlog-20261105.md`.
//!
//! On this platform the Lenovo services do not own an ETW channel or a
//! registered publisher: `wevtutil el`, `wevtutil ep` and the
//! `WINEVT\Publishers` registry all contain no Lenovo identity, and no
//! `EventLog\Application\<source>` key registers a Lenovo message file. The
//! only Lenovo records in the Windows event log are classic `Application`
//! channel records whose *provider string* is the service name
//! (`LenovoPcManagerService`). They carry event id 0, level 4 (information)
//! and no message resource: the structured message is empty. `wevtutil`'s text
//! formatter falls back to the generic event-id-0 text, which is operating
//! system text rather than Lenovo content and is never reported here.
//!
//! Consequences encoded in this module:
//!
//! * Providers are discovered from the machine (Lenovo service `ImagePath`
//!   values and Lenovo-registered event sources), never hardcoded.
//! * Selection uses an XPath `Provider[@Name='...']` filter. The provider
//!   name is only a string attribute of the record, so the classic
//!   `ProviderName=` filter cannot find these records at all.
//! * The record's raw `EventData/Data` strings are surfaced verbatim; no
//!   message text is synthesized from them.
//! * Every read is bounded and read-only: no clearing, no channel enabling,
//!   no writes.

use lctrl_core::{LctrlError, Result};
use serde::Serialize;

/// Upper bound on records one `vantage events` call may return.
pub const MAX_EVENT_LOG_RECORDS: usize = 200;

/// Default record count when the caller does not pass a bound.
pub const DEFAULT_EVENT_LOG_RECORDS: usize = 20;

/// Validate a caller-requested record count.
pub fn normalize_event_log_limit(limit: usize) -> Result<usize> {
    if (1..=MAX_EVENT_LOG_RECORDS).contains(&limit) {
        return Ok(limit);
    }
    Err(LctrlError::InvalidArgument {
        detail: format!(
            "the Lenovo event-log record count must be 1..={MAX_EVENT_LOG_RECORDS}; got {limit}"
        ),
    })
}

/// An event level with the raw Windows value preserved.
///
/// The vendor providers have no message resource, so Windows also has no
/// localized level name to offer; the documented level numbering is the only
/// identity available.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct EventLevel {
    pub value: u8,
    pub name: &'static str,
}

impl EventLevel {
    #[must_use]
    pub const fn from_raw(value: u8) -> Self {
        Self {
            value,
            name: match value {
                0 => "log_always",
                1 => "critical",
                2 => "error",
                3 => "warning",
                4 => "information",
                5 => "verbose",
                _ => "unknown",
            },
        }
    }
}

/// One raw event-log record attributed to a Lenovo provider.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LenovoEventRecord {
    /// Channel the record was read from (`Application` here).
    pub channel: String,
    /// Provider string stored in the record. It is not a registered publisher.
    pub provider: String,
    /// Event id (`EventID`), which is also the message id for these records.
    pub event_id: u32,
    /// Classic event qualifiers.
    pub qualifiers: u16,
    pub level: EventLevel,
    /// Event-record id inside the channel, so an operator can locate the record.
    pub record_id: u64,
    pub time_unix_ms: u64,
    /// `TimeCreated` in UTC, `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub time_utc: String,
    /// Raw `EventData/Data` strings in record order. Empty when the record has
    /// no insertion strings. No message text is synthesized from these.
    pub data: Vec<String>,
}

/// The bounded, read-only result of one `vantage events` query.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LenovoEventLogReport {
    /// Channels that accepted the query.
    pub channels: Vec<String>,
    /// Lenovo provider names discovered on this machine and used for the query.
    pub providers: Vec<String>,
    /// Newest first, at most `limit` records.
    pub events: Vec<LenovoEventRecord>,
    pub limit: usize,
}

/// Read-only access to the Lenovo records the OS event log holds.
pub trait EventLogControl: Send + Sync {
    /// Return at most `limit` Lenovo records, newest first.
    ///
    /// An empty `events` list with a non-empty `providers` list means the
    /// machine's providers exist but hold no records; both empty means no
    /// Lenovo provider/source was discoverable on this machine.
    fn lenovo_events(&self, limit: usize) -> Result<LenovoEventLogReport>;
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` for Unix milliseconds.
///
/// Howard Hinnant's `civil_from_days` is used so the HAL does not take a
/// calendar dependency just to render a timestamp.
#[must_use]
pub fn format_utc_millis(millis: u64) -> String {
    let millis = i64::try_from(millis).unwrap_or(i64::MAX);
    let days = millis.div_euclid(86_400_000);
    let within_day = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let hour = within_day / 3_600_000;
    let minute = (within_day % 3_600_000) / 60_000;
    let second = (within_day % 60_000) / 1_000;
    let millisecond = within_day % 1_000;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millisecond:03}Z")
}

/// `(year, month, day)` for a Unix day number (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month,
        day,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_EVENT_LOG_RECORDS, EventLevel, MAX_EVENT_LOG_RECORDS, format_utc_millis,
        normalize_event_log_limit,
    };

    #[test]
    fn limit_is_bounded_and_defaulted_by_the_caller() {
        assert_eq!(normalize_event_log_limit(1).unwrap(), 1);
        assert_eq!(
            normalize_event_log_limit(MAX_EVENT_LOG_RECORDS).unwrap(),
            MAX_EVENT_LOG_RECORDS
        );
        assert!(normalize_event_log_limit(0).is_err());
        assert!(normalize_event_log_limit(MAX_EVENT_LOG_RECORDS + 1).is_err());
        assert!(DEFAULT_EVENT_LOG_RECORDS >= 1);
    }

    #[test]
    fn documented_levels_keep_raw_value_and_name() {
        for (value, name) in [
            (0, "log_always"),
            (1, "critical"),
            (2, "error"),
            (3, "warning"),
            (4, "information"),
            (5, "verbose"),
        ] {
            let level = EventLevel::from_raw(value);
            assert_eq!(level.value, value);
            assert_eq!(level.name, name);
        }
        assert_eq!(EventLevel::from_raw(9).name, "unknown");
    }

    #[test]
    fn epoch_and_known_timestamps_render_as_utc() {
        assert_eq!(format_utc_millis(0), "1970-01-01T00:00:00.000Z");
        // 2026-11-03T19:09:41.072Z: the live LenovoPcManagerService record.
        assert_eq!(
            format_utc_millis(1_793_732_981_072),
            "2026-11-03T19:09:41.072Z"
        );
        // Leap day.
        assert_eq!(
            format_utc_millis(1_709_164_800_000),
            "2024-02-29T00:00:00.000Z"
        );
    }
}

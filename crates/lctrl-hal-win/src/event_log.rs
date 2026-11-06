//! Read-only Lenovo records from the Windows event log.
//!
//! Contract and the live 21VG observation: `docs/evidence/re-eventlog-20261105.md`.
//!
//! The Lenovo services on this platform do not own an ETW channel or a
//! registered publisher. Their only records are classic `Application` channel
//! entries whose provider string is the service name (`LenovoPcManagerService`),
//! with event id 0, level 4 (information), no message resource (the rendered
//! message is empty) and one raw `EventData/Data` string. Because the provider
//! string is not a registered publisher, only an XPath `Provider[@Name=...]`
//! filter finds them; the classic `ProviderName=` filter returns nothing.
//!
//! Provider names are therefore discovered from the machine itself — Lenovo
//! service `ImagePath` values plus event sources whose message file lives under
//! a Lenovo path — and the query is bounded in both directions: a hard record
//! cap, a fixed `EvtNext` batch, and a per-record render buffer that cannot
//! balloon. Nothing here writes, clears or enables a channel.

/// Hard cap on provider names one XPath may carry.
#[cfg(windows)]
pub(crate) const MAX_DISCOVERED_PROVIDERS: usize = 64;
/// Hard cap on registry child names read from one key level.
#[cfg(windows)]
pub(crate) const MAX_KEY_NAMES: usize = 2048;

/// Contexts that make a provider-name XPath possible.
#[cfg_attr(not(windows), allow(dead_code))] // exercised on host by its tests
mod event_log_pure {
    /// True when a service image path or event-source message file belongs to a
    /// Lenovo installation.
    #[must_use]
    pub fn is_lenovo_path(path: &str) -> bool {
        path.to_ascii_lowercase().contains("lenovo")
    }

    /// File stem of a service `ImagePath`, with quotes and arguments stripped.
    ///
    /// The stem is the name a service most plausibly writes as its provider
    /// string; the service key name is queried too, so both spellings are
    /// covered without hardcoding either.
    #[must_use]
    pub fn image_stem(image_path: &str) -> Option<String> {
        let trimmed = image_path.trim();
        let unquoted = trimmed
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .unwrap_or(trimmed);
        let file = unquoted.rsplit(['\\', '/']).next()?.trim();
        let file = file.split_whitespace().next().unwrap_or(file);
        let stem = file.split('.').next()?.trim();
        (!stem.is_empty()).then(|| stem.to_string())
    }

    /// XPath selecting records whose provider string is one of `names`.
    ///
    /// Returns `None` when no name survives validation, so the caller reports
    /// zero sources instead of issuing an empty predicate.
    #[must_use]
    pub fn provider_xpath(names: &[String]) -> Option<String> {
        let mut predicate = String::from("*[System[");
        let mut first = true;
        for name in names.iter().filter(|name| is_safe_provider_name(name)) {
            if !first {
                predicate.push_str(" or ");
            }
            first = false;
            predicate.push_str("Provider[@Name='");
            predicate.push_str(name);
            predicate.push_str("']");
        }
        if first {
            return None;
        }
        predicate.push_str("]]");
        Some(predicate)
    }

    /// A provider name is embedded in an XPath literal, so it may only contain
    /// characters that cannot terminate or reshape the expression.
    #[must_use]
    pub fn is_safe_provider_name(name: &str) -> bool {
        !name.is_empty()
            && name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b' ')
            })
    }

    /// Case-insensitive de-duplication with a deterministic order and a hard cap.
    #[must_use]
    pub fn dedup_provider_names(names: Vec<String>, cap: usize) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut deduped = Vec::new();
        for name in names {
            if !is_safe_provider_name(&name) {
                continue;
            }
            if seen.insert(name.to_ascii_lowercase()) {
                deduped.push(name);
            }
            if deduped.len() >= cap {
                break;
            }
        }
        deduped.sort_by_key(|name| name.to_ascii_lowercase());
        deduped
    }

    /// `VT_FILETIME` (100 ns ticks since 1601-01-01 UTC) as Unix milliseconds.
    #[must_use]
    pub fn unix_ms_from_filetime(filetime: u64) -> Option<u64> {
        /// 1970-01-01 minus 1601-01-01 in 100 ns ticks.
        const EPOCH_DIFFERENCE_TICKS: i128 = 116_444_736_000_000_000;
        let ticks = i128::from(filetime) - EPOCH_DIFFERENCE_TICKS;
        u64::try_from(ticks / 10_000).ok()
    }
}

#[cfg(windows)]
mod native {
    use std::ffi::c_void;

    use lctrl_core::{Availability, CapabilitySet, LctrlError, Result};
    use lctrl_hal::{
        EventLevel, EventLogControl, LenovoEventLogReport, LenovoEventRecord, format_utc_millis,
        normalize_event_log_limit,
    };
    use windows_sys::Win32::Foundation::{
        ERROR_EVT_CHANNEL_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS, ERROR_TIMEOUT,
        GetLastError,
    };
    use windows_sys::Win32::System::EventLog::{
        EVT_HANDLE, EVT_VARIANT, EVT_VARIANT_TYPE_ARRAY, EVT_VARIANT_TYPE_MASK, EvtClose, EvtNext,
        EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderEventValues,
        EvtSystemChannel, EvtSystemEventID, EvtSystemEventRecordId, EvtSystemLevel,
        EvtSystemPropertyIdEND, EvtSystemProviderName, EvtSystemQualifiers, EvtSystemTimeCreated,
        EvtVarTypeString,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegEnumKeyExW,
        RegOpenKeyExW, RegQueryValueExW,
    };

    use super::event_log_pure::{
        dedup_provider_names, is_lenovo_path, image_stem, provider_xpath, unix_ms_from_filetime,
    };
    use super::{MAX_DISCOVERED_PROVIDERS, MAX_KEY_NAMES};
    use crate::map_win_error;

    const SERVICES_KEY: &str = r"SYSTEM\CurrentControlSet\Services";
    const EVENT_LOG_KEY: &str = r"SYSTEM\CurrentControlSet\Services\EventLog";
    /// Channels that can carry classic Lenovo records; a missing channel is
    /// skipped, never synthesized.
    const CHANNELS: &[&str] = &["Application", "System"];
    /// One `EvtNext` batch. Combined with the record cap this bounds a query.
    const NEXT_BATCH: usize = 16;
    const NEXT_TIMEOUT_MS: u32 = 250;
    /// Upper bound for one rendered record. Classic records with a handful of
    /// insertion strings stay far below this.
    const MAX_RENDER_BYTES: u32 = 256 * 1024;
    /// Upper bound for one registry string (service `ImagePath`, message file).
    const MAX_REGISTRY_BYTES: u32 = 4096;
    /// Index of the first user-data variant in a rendered record.
    const FIRST_USER_PROPERTY: usize = EvtSystemPropertyIdEND as usize;

    /// Read-only event-log access through wevtapi on this machine.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct WindowsEventLog;

    impl EventLogControl for WindowsEventLog {
        fn lenovo_events(&self, limit: usize) -> Result<LenovoEventLogReport> {
            let limit = normalize_event_log_limit(limit)?;
            let providers = discover_providers()?;
            let Some(query) = provider_xpath(&providers) else {
                return Ok(LenovoEventLogReport {
                    channels: Vec::new(),
                    providers,
                    events: Vec::new(),
                    limit,
                });
            };
            let mut channels = Vec::new();
            let mut events = Vec::new();
            for channel in CHANNELS {
                let Some(mut records) = query_channel(channel, &query, limit)? else {
                    continue;
                };
                channels.push((*channel).to_string());
                events.append(&mut records);
            }
            events.sort_by(|left, right| {
                right
                    .time_unix_ms
                    .cmp(&left.time_unix_ms)
                    .then_with(|| right.record_id.cmp(&left.record_id))
            });
            events.truncate(limit);
            Ok(LenovoEventLogReport {
                channels,
                providers,
                events,
                limit,
            })
        }
    }

    /// Discover the provider strings the Lenovo software on this machine can
    /// write: service key names and image stems of Lenovo services, plus
    /// registered event sources whose message file is under a Lenovo path.
    fn discover_providers() -> Result<Vec<String>> {
        let mut names = Vec::new();
        for (service, path) in service_image_paths()? {
            if !is_lenovo_path(&path) {
                continue;
            }
            names.push(service);
            if let Some(stem) = image_stem(&path) {
                names.push(stem);
            }
        }
        for (source, message_file) in registered_event_sources()? {
            if is_lenovo_path(&message_file) {
                names.push(source);
            }
        }
        Ok(dedup_provider_names(names, MAX_DISCOVERED_PROVIDERS))
    }

    fn service_image_paths() -> Result<Vec<(String, String)>> {
        let services = open_registry_key(SERVICES_KEY)?;
        let mut paths = Vec::new();
        for name in registry_subkeys(services.0)? {
            let Ok(service) = open_subkey(services.0, &name) else {
                continue;
            };
            if let Some(path) = registry_string(service.0, "ImagePath") {
                paths.push((name, path));
            }
        }
        Ok(paths)
    }

    fn registered_event_sources() -> Result<Vec<(String, String)>> {
        let event_log = open_registry_key(EVENT_LOG_KEY)?;
        let mut sources = Vec::new();
        for channel in registry_subkeys(event_log.0)? {
            let Ok(channel_key) = open_subkey(event_log.0, &channel) else {
                continue;
            };
            let Ok(names) = registry_subkeys(channel_key.0) else {
                continue;
            };
            for source in names {
                let Ok(key) = open_subkey(channel_key.0, &source) else {
                    continue;
                };
                let message_file = registry_string(key.0, "EventMessageFile")
                    .or_else(|| registry_string(key.0, "ParameterMessageFile"))
                    .or_else(|| registry_string(key.0, "CategoryMessageFile"));
                if let Some(path) = message_file {
                    sources.push((source, path));
                }
            }
        }
        Ok(sources)
    }

    /// Run one bounded reverse query on `channel`; `None` means the channel does
    /// not exist on this machine.
    fn query_channel(
        channel: &str,
        query: &str,
        limit: usize,
    ) -> Result<Option<Vec<LenovoEventRecord>>> {
        let channel_wide = wide(channel);
        let query_wide = wide(query);
        let handle = unsafe {
            EvtQuery(
                0,
                channel_wide.as_ptr(),
                query_wide.as_ptr(),
                EvtQueryChannelPath | EvtQueryReverseDirection,
            )
        };
        if handle == 0 {
            let status = unsafe { GetLastError() };
            if status == ERROR_EVT_CHANNEL_NOT_FOUND {
                return Ok(None);
            }
            return Err(map_event_log_error(
                status,
                &format!("query the {channel} channel"),
            ));
        }
        let query_handle = EvtHandle(handle);
        let mut records = Vec::new();
        let mut batch = [0isize; NEXT_BATCH];
        while records.len() < limit {
            let mut returned = 0u32;
            let ok = unsafe {
                EvtNext(
                    query_handle.0,
                    batch.len() as u32,
                    batch.as_mut_ptr(),
                    NEXT_TIMEOUT_MS,
                    0,
                    &mut returned,
                )
            };
            if ok == 0 {
                let status = unsafe { GetLastError() };
                if status == ERROR_NO_MORE_ITEMS || status == ERROR_TIMEOUT {
                    break;
                }
                return Err(map_event_log_error(
                    status,
                    &format!("read the {channel} channel"),
                ));
            }
            if returned == 0 {
                break;
            }
            // Every handle in the batch is released even when the cap is
            // reached mid-batch or one record fails to render, so a bounded
            // call cannot leak events; the first render failure is kept and
            // reported after the batch has been closed.
            let mut rendered = Vec::with_capacity(returned as usize);
            let mut failure = None;
            for &event in batch.iter().take(returned as usize) {
                let record = render_record(event, channel);
                unsafe { EvtClose(event) };
                match record {
                    Ok(record) => rendered.push(record),
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            }
            if let Some(error) = failure {
                return Err(error);
            }
            let remaining = limit - records.len();
            records.extend(rendered.into_iter().take(remaining));
        }
        Ok(Some(records))
    }

    fn render_record(event: EVT_HANDLE, channel: &str) -> Result<LenovoEventRecord> {
        let mut buffer: Vec<u64> = vec![0; 64];
        let mut used = 0u32;
        let mut count = 0u32;
        loop {
            let size = u32::try_from(buffer.len() * std::mem::size_of::<u64>()).map_err(|_| {
                LctrlError::ChannelUnavailable {
                    channel: "event render buffer size".into(),
                }
            })?;
            let ok = unsafe {
                EvtRender(
                    0,
                    event,
                    EvtRenderEventValues,
                    size,
                    buffer.as_mut_ptr().cast::<c_void>(),
                    &mut used,
                    &mut count,
                )
            };
            if ok != 0 {
                break;
            }
            let status = unsafe { GetLastError() };
            if status != ERROR_INSUFFICIENT_BUFFER {
                return Err(map_event_log_error(status, "render an event record"));
            }
            if used == 0 || used > MAX_RENDER_BYTES {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("event record needs {used} render bytes"),
                });
            }
            let units = usize::try_from(used).unwrap_or(0).div_ceil(8) + 8;
            if units <= buffer.len() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "event render buffer did not grow".into(),
                });
            }
            buffer.resize(units, 0);
        }
        let property_count = usize::try_from(count).unwrap_or(0);
        if property_count < FIRST_USER_PROPERTY {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("event record rendered {property_count} properties"),
            });
        }
        // Every variant read below indexes the render buffer, so the count the
        // renderer reported must fit the bytes it reported using (and the
        // buffer itself). A record that disagrees is refused, not read past.
        let rendered_bytes = usize::try_from(used).unwrap_or(0);
        let variants_bytes = property_count.saturating_mul(std::mem::size_of::<EVT_VARIANT>());
        if rendered_bytes < variants_bytes
            || buffer.len() * std::mem::size_of::<u64>() < variants_bytes
        {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "event record claims {property_count} properties in {rendered_bytes} rendered bytes"
                ),
            });
        }
        let values = buffer.as_ptr().cast::<EVT_VARIANT>();
        let provider = variant_string(values, EvtSystemProviderName as usize).unwrap_or_default();
        let event_id = variant_u16(values, EvtSystemEventID as usize).map_or(0, u32::from);
        let qualifiers = variant_u16(values, EvtSystemQualifiers as usize).unwrap_or(0);
        let level = variant_u8(values, EvtSystemLevel as usize).unwrap_or(0);
        let time_created = variant_filetime(values, EvtSystemTimeCreated as usize).unwrap_or(0);
        let record_id = variant_u64(values, EvtSystemEventRecordId as usize).unwrap_or(0);
        let rendered_channel = variant_string(values, EvtSystemChannel as usize)
            .unwrap_or_else(|| channel.to_string());
        let mut data = Vec::new();
        for index in FIRST_USER_PROPERTY..property_count {
            if let Some(value) = variant_string(values, index) {
                data.push(value);
            } else if let Some(array) = variant_string_array(values, index) {
                data.extend(array);
            }
        }
        let time_unix_ms = unix_ms_from_filetime(time_created).unwrap_or(0);
        Ok(LenovoEventRecord {
            channel: rendered_channel,
            provider,
            event_id,
            qualifiers,
            level: EventLevel::from_raw(level),
            record_id,
            time_unix_ms,
            time_utc: format_utc_millis(time_unix_ms),
            data,
        })
    }

    fn variant_type(values: *const EVT_VARIANT, index: usize) -> u32 {
        // SAFETY: callers pass indices below the record's property count, and
        // the buffer they point into outlives every value read from it.
        unsafe { (*values.add(index)).Type }
    }

    fn variant_string(values: *const EVT_VARIANT, index: usize) -> Option<String> {
        if variant_type(values, index) & EVT_VARIANT_TYPE_MASK != EvtVarTypeString as u32 {
            return None;
        }
        // SAFETY: the variant type was just checked to be a string scalar.
        let pointer = unsafe { (*values.add(index)).Anonymous.StringVal };
        read_wide_string(pointer)
    }

    fn variant_string_array(values: *const EVT_VARIANT, index: usize) -> Option<Vec<String>> {
        let variant_type = variant_type(values, index);
        if variant_type & EVT_VARIANT_TYPE_ARRAY == 0
            || variant_type & EVT_VARIANT_TYPE_MASK != EvtVarTypeString as u32
        {
            return None;
        }
        // SAFETY: the variant type was just checked to be a string array.
        let (count, array) = unsafe {
            let variant = &*values.add(index);
            (variant.Count, variant.Anonymous.StringArr)
        };
        if array.is_null() {
            return None;
        }
        let count = usize::try_from(count).ok().filter(|count| *count <= 64)?;
        let mut rendered = Vec::with_capacity(count);
        for offset in 0..count {
            // SAFETY: the array carries `count` pointers, per the variant header.
            let pointer = unsafe { *array.add(offset) };
            rendered.push(read_wide_string(pointer)?);
        }
        Some(rendered)
    }

    fn read_wide_string(pointer: *const u16) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        let mut length = 0usize;
        // SAFETY: a string variant is NUL-terminated; the scan is bounded so a
        // malformed record cannot walk out of the render buffer.
        while unsafe { *pointer.add(length) } != 0 {
            length += 1;
            if length > 4096 {
                return None;
            }
        }
        // SAFETY: `length` counts the units before the NUL just located.
        let units = unsafe { std::slice::from_raw_parts(pointer, length) };
        Some(String::from_utf16_lossy(units))
    }

    fn variant_u8(values: *const EVT_VARIANT, index: usize) -> Option<u8> {
        // SAFETY: an 8-bit system property is stored as a byte value.
        unsafe { Some((*values.add(index)).Anonymous.ByteVal) }
    }

    fn variant_u16(values: *const EVT_VARIANT, index: usize) -> Option<u16> {
        // SAFETY: a 16-bit system property is stored as a word value.
        unsafe { Some((*values.add(index)).Anonymous.UInt16Val) }
    }

    fn variant_u64(values: *const EVT_VARIANT, index: usize) -> Option<u64> {
        // SAFETY: a 64-bit system property is stored as a qword value.
        unsafe { Some((*values.add(index)).Anonymous.UInt64Val) }
    }

    fn variant_filetime(values: *const EVT_VARIANT, index: usize) -> Option<u64> {
        // SAFETY: `TimeCreated` is a FILETIME stored as a 64-bit value.
        unsafe { Some((*values.add(index)).Anonymous.FileTimeVal) }
    }

    /// NUL-terminated UTF-16 for a runtime string.
    ///
    /// The crate's `wide!` macro only accepts literals; registry paths, channel
    /// names and the generated XPath are runtime data, so they are widened here.
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn map_event_log_error(status: u32, context: &str) -> LctrlError {
        match status {
            // The provider filter was rejected as a malformed XPath.
            15001 => LctrlError::InvalidArgument {
                detail: format!("{context}: the provider filter was rejected"),
            },
            // The channel does not exist on this machine.
            15007 => LctrlError::ChannelUnavailable {
                channel: format!("{context}: channel not found"),
            },
            other => map_win_error(other, context),
        }
    }

    /// Read-only capability probe behind `info`/`capability`.
    ///
    /// Cheap by construction: it discovers provider names and opens the
    /// `Application` query, then closes it. No event is read and nothing is
    /// cleared or enabled. See `docs/evidence/re-eventlog-20261105.md`.
    pub(crate) fn record_event_log_capability(capabilities: &mut CapabilitySet) -> Result<()> {
        match probe_event_log() {
            Ok(providers) => {
                capabilities.record(
                    "events.log",
                    Availability::Available,
                    Some(format!(
                        "classic Application-channel records carry the provider string; \
                         {providers} Lenovo provider name(s) discovered from service ImagePath \
                         values and registered event sources"
                    )),
                )?;
                capabilities.record(
                    "channel.events",
                    Availability::Available,
                    Some(
                        "wevtapi channel query opened read-only; no Lenovo ETW channel or \
                         publisher is registered on this machine"
                            .into(),
                    ),
                )?;
            }
            Err(error) => {
                let detail = error.to_string();
                capabilities.record("events.log", Availability::Unavailable, Some(detail.clone()))?;
                capabilities.record("channel.events", Availability::Unavailable, Some(detail))?;
            }
        }
        Ok(())
    }

    fn probe_event_log() -> Result<usize> {
        let providers = discover_providers()?;
        let channel = wide("Application");
        let query = wide("*");
        let handle =
            unsafe { EvtQuery(0, channel.as_ptr(), query.as_ptr(), EvtQueryChannelPath) };
        if handle == 0 {
            return Err(map_event_log_error(
                unsafe { GetLastError() },
                "open the Application channel",
            ));
        }
        drop(EvtHandle(handle));
        Ok(providers.len())
    }

    /// Owns one wevtapi handle so every early return closes it.
    struct EvtHandle(EVT_HANDLE);

    impl Drop for EvtHandle {
        fn drop(&mut self) {
            unsafe { EvtClose(self.0) };
        }
    }

    /// Owns one registry key so every early return closes it.
    struct RegKey(HKEY);

    impl Drop for RegKey {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn open_registry_key(path: &str) -> Result<RegKey> {
        let path_wide = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, path_wide.as_ptr(), 0, KEY_READ, &mut key)
        };
        if status != 0 {
            return Err(map_win_error(status, &format!("open HKLM\\{path}")));
        }
        Ok(RegKey(key))
    }

    fn open_subkey(parent: HKEY, name: &str) -> Result<RegKey> {
        let name_wide = wide(name);
        let mut key: HKEY = std::ptr::null_mut();
        let status = unsafe { RegOpenKeyExW(parent, name_wide.as_ptr(), 0, KEY_READ, &mut key) };
        if status != 0 {
            return Err(map_win_error(status, name));
        }
        Ok(RegKey(key))
    }

    fn registry_subkeys(key: HKEY) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let mut index = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                return Ok(names);
            }
            if status != 0 {
                return Err(map_win_error(status, "enumerate registry subkeys"));
            }
            index += 1;
            names.push(String::from_utf16_lossy(&name[..length as usize]));
            if names.len() >= MAX_KEY_NAMES {
                return Ok(names);
            }
        }
    }

    /// One registry string value. A missing or non-string value is simply
    /// absent from discovery; it is not an error for the whole command.
    fn registry_string(key: HKEY, value: &str) -> Option<String> {
        let value_wide = wide(value);
        let mut buffer = [0u16; (MAX_REGISTRY_BYTES / 2) as usize];
        let mut bytes = MAX_REGISTRY_BYTES;
        let mut kind = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                value_wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast::<u8>(),
                &mut bytes,
            )
        };
        if status != 0 || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
            return None;
        }
        let units = (bytes as usize / 2).min(buffer.len());
        let end = buffer[..units]
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units);
        let text = String::from_utf16_lossy(&buffer[..end]);
        (!text.is_empty()).then_some(text)
    }
}

#[cfg(windows)]
pub use native::WindowsEventLog;

#[cfg(windows)]
pub(crate) use native::record_event_log_capability;

#[cfg(not(windows))]
#[allow(dead_code)] // Only the Windows composition root can call recorder.
pub(crate) fn record_event_log_capability(
    capabilities: &mut lctrl_core::CapabilitySet,
) -> lctrl_core::Result<()> {
    for feature in ["events.log", "channel.events"] {
        capabilities.record(
            feature,
            lctrl_core::Availability::Unavailable,
            Some("the Lenovo event log is a Windows facility".into()),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::event_log_pure::{
        dedup_provider_names, image_stem, is_lenovo_path, provider_xpath, unix_ms_from_filetime,
    };

    #[test]
    fn lenovo_paths_are_recognized_case_insensitively() {
        assert!(is_lenovo_path(
            r"C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\LenovoPcManagerService.exe"
        ));
        assert!(is_lenovo_path(
            r"C:\WINDOWS\System32\DriverStore\FileRepository\lenovofnandfunctionkeys.inf_amd64_5e21bf389d23855a\LenovoUtilityService.exe"
        ));
        assert!(is_lenovo_path(r"C:\WINDOWS\System32\drivers\Lenovo\udc\Service\UDClientService.exe"));
        assert!(!is_lenovo_path(r"C:\Windows\System32\svchost.exe -k netsvcs"));
        assert!(!is_lenovo_path(""));
    }

    #[test]
    fn image_stem_strips_quotes_arguments_and_extension() {
        assert_eq!(
            image_stem(r#""C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\LenovoPcManagerService.exe""#)
                .as_deref(),
            Some("LenovoPcManagerService")
        );
        assert_eq!(
            image_stem(r#""C:\Program Files (x86)\Lenovo\PCManager\x64\wsctrl11.exe" /svc_run"#)
                .as_deref(),
            Some("wsctrl11")
        );
        assert_eq!(
            image_stem(r"C:\WINDOWS\System32\DriverStore\FileRepository\lnvprocessmanagement.inf_amd64_863cdf71782696c8\LNVDispatcherService.exe")
                .as_deref(),
            Some("LNVDispatcherService")
        );
        assert_eq!(image_stem(""), None);
    }

    #[test]
    fn provider_xpath_filters_on_the_provider_name() {
        let names = vec!["LenovoPcManagerService".to_string(), "LNVDispatcherService".into()];
        assert_eq!(
            provider_xpath(&names).as_deref(),
            Some("*[System[Provider[@Name='LenovoPcManagerService'] or Provider[@Name='LNVDispatcherService']]]")
        );
        assert!(provider_xpath(&[]).is_none());
        assert!(provider_xpath(&["bad'name".to_string()]).is_none());
    }

    #[test]
    fn provider_names_dedup_case_insensitively_and_bound_the_xpath() {
        let names = vec![
            "LenovoPcManagerService".to_string(),
            "lenovopcmanagerservice".to_string(),
            "LNVDispatcherService".into(),
        ];
        let deduped = dedup_provider_names(names, 8);
        assert_eq!(deduped, vec!["LenovoPcManagerService", "LNVDispatcherService"]);
        assert_eq!(dedup_provider_names(vec!["A".into(), "B".into()], 1).len(), 1);
    }

    #[test]
    fn filetime_converts_to_unix_milliseconds() {
        // 2026-11-03T18:09:41.072Z from the live LenovoPcManagerService record.
        // A FILETIME counts 100 ns ticks, so one Unix millisecond is 10 000.
        let filetime = 17_937_329_810_720_000 + 116_444_736_000_000_000;
        assert_eq!(unix_ms_from_filetime(filetime), Some(1_793_732_981_072));
        assert_eq!(unix_ms_from_filetime(0), None);
    }
}

//! Camera consent and last-use records from the Windows capability consent
//! store.
//!
//! Contract: `docs/evidence/re-camera-inuse-20261106.md`. The store is
//! `…\CapabilityAccessManager\ConsentStore\webcam` under `HKCU` and `HKLM`,
//! with per-application subkeys carrying a `Value` (`Allow`/`Deny`/
//! `PromptOnFirstUse`) and FILETIME `LastUsedTimeStart`/`LastUsedTimeStop`
//! stamps; non-packaged programs live under `\NonPackaged\`, with every
//! backslash of the executable path stored as `#`. Policy lives in a separate key
//! (`SOFTWARE\Policies\Microsoft\Windows\AppPrivacy`) and is reported as its own
//! field, never merged into the store.
//!
//! Every key here is opened `KEY_READ`. No value is written, no device is
//! opened, and no capture session is started: the answer is what Windows
//! recorded, not whether a camera is streaming.

use std::ptr;

use lctrl_core::{
    CameraAppPrivacyPolicy, CameraConsentEntry, CameraConsentReport, CameraConsentScope,
    CameraConsentScopeReport, CameraPrivacyPolicy, CameraStamp, LctrlError, Result,
    parse_camera_consent,
};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_DWORD, REG_MULTI_SZ,
        REG_QWORD, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegEnumKeyExW, RegOpenKeyExW,
        RegQueryValueExW,
    },
};

/// The per-capability consent container shared by both hives.
const CONSENT_ROOT: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam";
/// Container of non-packaged programs, whose children are encoded paths.
const NON_PACKAGED: &str = "NonPackaged";
/// Documented policy key; absent on a machine with no camera app-privacy policy.
const POLICY_ROOT: &str = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
const VALUE_NAME: &str = "Value";
const LAST_SET: &str = "LastSetTime";
const LAST_USED_START: &str = "LastUsedTimeStart";
const LAST_USED_STOP: &str = "LastUsedTimeStop";
/// Documented `LetAppsAccessCamera` policy value (0 user in control, 1 force
/// allow, 2 force deny) and its per-app group-policy lists.
const LET_APPS_ACCESS_CAMERA: &str = "LetAppsAccessCamera";
const FORCE_ALLOW: &str = "LetAppsAccessCamera_ForceAllowTheseApps";
const FORCE_DENY: &str = "LetAppsAccessCamera_ForceDenyTheseApps";

/// Bound on one registry payload. A consent record is a short string or a
/// 64-bit stamp; anything larger is treated as a malformed channel rather than
/// copied.
const MAX_VALUE_BYTES: u32 = 64 * 1024;
/// Bound on one enumerated subkey name (encoded non-packaged paths are long).
const MAX_SUBKEY_CHARS: usize = 2048;

/// Read every camera consent container plus the app-privacy policy.
///
/// A missing container is reported as absent; a missing policy key is reported
/// as `present: false`. Only an actual registry failure fails the read.
pub(crate) fn read_camera_consent() -> Result<CameraConsentReport> {
    let non_packaged = format!("{CONSENT_ROOT}\\{NON_PACKAGED}");
    let mut scopes = Vec::with_capacity(4);
    for (scope, root, path) in [
        (
            CameraConsentScope::CurrentUser,
            HKEY_CURRENT_USER,
            CONSENT_ROOT,
        ),
        (
            CameraConsentScope::CurrentUserNonPackaged,
            HKEY_CURRENT_USER,
            non_packaged.as_str(),
        ),
        (CameraConsentScope::Machine, HKEY_LOCAL_MACHINE, CONSENT_ROOT),
        (
            CameraConsentScope::MachineNonPackaged,
            HKEY_LOCAL_MACHINE,
            non_packaged.as_str(),
        ),
    ] {
        scopes.push(read_scope(scope, root, path)?);
    }
    Ok(CameraConsentReport::new(scopes, read_policy()?))
}

fn read_scope(
    scope: CameraConsentScope,
    root: HKEY,
    path: &str,
) -> Result<CameraConsentScopeReport> {
    let Some(key) = Key::open(root, path)? else {
        return Ok(CameraConsentScopeReport {
            scope,
            present: false,
            consent: None,
            consent_raw: None,
            last_set: None,
            entries: Vec::new(),
            notes: Vec::new(),
        });
    };
    let mut notes = Vec::new();
    let consent_text = noted(key.string(VALUE_NAME), VALUE_NAME, &mut notes);
    let (consent, consent_raw) = parse_camera_consent(consent_text.as_deref());
    let last_set = noted(key.qword(LAST_SET), LAST_SET, &mut notes).and_then(CameraStamp::new);
    let names = key.subkeys(&mut notes);
    let mut entries = Vec::with_capacity(names.len());
    for name in names {
        // `NonPackaged` is the sibling container read on its own; it is not an
        // application record.
        if !scope.non_packaged() && name.eq_ignore_ascii_case(NON_PACKAGED) {
            continue;
        }
        match read_record(&key, scope, &name) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => notes.push(format!("{name}: key disappeared during enumeration")),
            Err(error) => notes.push(format!("{name}: {error}")),
        }
    }
    // Registry enumeration order is stable in practice but not contractual;
    // sorting keeps two consecutive reads byte-identical.
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(CameraConsentScopeReport {
        scope,
        present: true,
        consent,
        consent_raw,
        last_set,
        entries,
        notes,
    })
}

/// Unwrap one value read; a malformed value becomes a note instead of failing
/// the whole report, so one bad record cannot hide the rest of the store.
fn noted<T>(result: Result<Option<T>>, name: &str, notes: &mut Vec<String>) -> Option<T> {
    match result {
        Ok(value) => value,
        Err(error) => {
            notes.push(format!("{name}: {error}"));
            None
        }
    }
}

/// Read one application record. A record that cannot be read is a note on the
/// container, not a failed report and not a dropped entry.
fn read_record(
    container: &Key,
    scope: CameraConsentScope,
    name: &str,
) -> Result<Option<CameraConsentEntry>> {
    let Some(key) = container.open_child(name)? else {
        return Ok(None);
    };
    let consent_text = key.string(VALUE_NAME)?;
    Ok(Some(CameraConsentEntry::new(
        scope,
        name,
        consent_text.as_deref(),
        key.qword(LAST_SET)?,
        key.qword(LAST_USED_START)?,
        key.qword(LAST_USED_STOP)?,
    )))
}

fn read_policy() -> Result<CameraPrivacyPolicy> {
    let Some(key) = Key::open(HKEY_LOCAL_MACHINE, POLICY_ROOT)? else {
        // No policy key: report absence. "User in control" is the documented
        // default *of a configured policy*, not evidence about this machine.
        return Ok(CameraPrivacyPolicy::default());
    };
    let mut notes = Vec::new();
    let value = noted(key.dword(LET_APPS_ACCESS_CAMERA), LET_APPS_ACCESS_CAMERA, &mut notes);
    Ok(CameraPrivacyPolicy {
        present: true,
        let_apps_access_camera: value.and_then(CameraAppPrivacyPolicy::from_raw),
        let_apps_access_camera_value: value,
        force_allow: noted(key.multi_string(FORCE_ALLOW), FORCE_ALLOW, &mut notes),
        force_deny: noted(key.multi_string(FORCE_DENY), FORCE_DENY, &mut notes),
        notes,
    })
}

/// Owned `KEY_READ` handle that closes on drop.
struct Key(HKEY);

impl Key {
    /// `Ok(None)` when the key does not exist.
    fn open(root: HKEY, path: &str) -> Result<Option<Self>> {
        let path_wide = wide(path);
        let mut handle: HKEY = ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(root, path_wide.as_ptr(), 0, KEY_READ, &mut handle) };
        match status {
            0 => Ok(Some(Self(handle))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            status => Err(crate::map_win_error(status, &format!("open {path}"))),
        }
    }

    fn open_child(&self, name: &str) -> Result<Option<Self>> {
        Self::open(self.0, name)
    }

    /// One value, as its declared type and raw bytes.
    fn value(&self, name: &str) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>> {
        let name_wide = wide(name);
        let mut kind: REG_VALUE_TYPE = 0;
        let mut bytes: u32 = 0;
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name_wide.as_ptr(),
                ptr::null(),
                &mut kind,
                ptr::null_mut(),
                &mut bytes,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 {
            return Err(crate::map_win_error(
                status,
                &format!("size of registry value {name}"),
            ));
        }
        if bytes > MAX_VALUE_BYTES {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} declares {bytes} bytes; refusing to read past {MAX_VALUE_BYTES}"
                ),
            });
        }
        let mut buffer = vec![0u8; bytes.max(1) as usize];
        let mut read = bytes;
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name_wide.as_ptr(),
                ptr::null(),
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut read,
            )
        };
        if status != 0 {
            return Err(crate::map_win_error(
                status,
                &format!("registry value {name}"),
            ));
        }
        if read > bytes {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} grew from {bytes} to {read} bytes between reads"
                ),
            });
        }
        buffer.truncate(read as usize);
        Ok(Some((kind, buffer)))
    }

    fn string(&self, name: &str) -> Result<Option<String>> {
        let Some((kind, bytes)) = self.value(name)? else {
            return Ok(None);
        };
        if kind != REG_SZ || bytes.len() % 2 != 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} is type {kind} with {} bytes; expected REG_SZ",
                    bytes.len()
                ),
            });
        }
        Ok(Some(decode_utf16(&bytes)))
    }

    fn qword(&self, name: &str) -> Result<Option<u64>> {
        let Some((kind, bytes)) = self.value(name)? else {
            return Ok(None);
        };
        if kind != REG_QWORD || bytes.len() != 8 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} is type {kind} with {} bytes; expected REG_QWORD",
                    bytes.len()
                ),
            });
        }
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes);
        Ok(Some(u64::from_le_bytes(raw)))
    }

    fn dword(&self, name: &str) -> Result<Option<u32>> {
        let Some((kind, bytes)) = self.value(name)? else {
            return Ok(None);
        };
        if kind != REG_DWORD || bytes.len() != 4 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} is type {kind} with {} bytes; expected REG_DWORD",
                    bytes.len()
                ),
            });
        }
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&bytes);
        Ok(Some(u32::from_le_bytes(raw)))
    }

    fn multi_string(&self, name: &str) -> Result<Option<Vec<String>>> {
        let Some((kind, bytes)) = self.value(name)? else {
            return Ok(None);
        };
        if kind != REG_MULTI_SZ || bytes.len() % 2 != 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "registry value {name} is type {kind} with {} bytes; expected REG_MULTI_SZ",
                    bytes.len()
                ),
            });
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        Ok(Some(
            units
                .split(|unit| *unit == 0)
                .filter(|run| !run.is_empty())
                .map(|run| String::from_utf16_lossy(run))
                .collect(),
        ))
    }

    /// Enumerated subkey names. A name that cannot be read becomes a note.
    fn subkeys(&self, notes: &mut Vec<String>) -> Vec<String> {
        let mut names = Vec::new();
        let mut index = 0u32;
        loop {
            let mut buffer = [0u16; MAX_SUBKEY_CHARS];
            let mut length = MAX_SUBKEY_CHARS as u32;
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    buffer.as_mut_ptr(),
                    &mut length,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                return names;
            }
            if status != 0 {
                notes.push(format!(
                    "subkey at index {index}: {}",
                    crate::map_win_error(status, "RegEnumKeyExW")
                ));
                return names;
            }
            index += 1;
            names.push(String::from_utf16_lossy(
                &buffer[..(length as usize).min(MAX_SUBKEY_CHARS)],
            ));
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

/// Decode a `REG_SZ` payload, stopping at the terminator Windows may or may not
/// include.
fn decode_utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let end = units.iter().position(|unit| *unit == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// NUL-terminated UTF-16 for the Win32 wide APIs. Every path and value name used
/// here is ASCII, so one `u16` per byte suffices.
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

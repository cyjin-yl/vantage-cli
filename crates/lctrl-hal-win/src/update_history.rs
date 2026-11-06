//! Read-only access to the update history the Lenovo System Update add-in
//! keeps on this machine.
//!
//! The add-in (`LenovoSystemUpdateAddin`) records what it installed in two
//! SQLite ledgers under
//! `%ProgramData%\Lenovo\Vantage\AddinData\LenovoSystemUpdateAddin\session\`:
//!
//! * `editable_update_history.db` — one row per recorded install:
//!   `id, packagename, packagedesc, version, utcinstalldate, status,
//!   packagesize, packagereleasedate, reboottype, crc, hardwareid`. The install
//!   time is .NET `DateTime` ticks (100 ns since 0001-01-01).
//! * `update_history.db` — one row per catalogue package the add-in evaluated:
//!   `id, title, status, version, additionalinfo, severity, installdate,
//!   hardwareid`. It carries no real install time (`installdate` is empty).
//!
//! Both are read through the operating system's own SQLite engine
//! (`winsqlite3.dll`, shipped in `System32` on Windows 10 1709+ and on the
//! 21VG target), opened `READONLY`. Nothing here writes, prunes, or reorders a
//! ledger, and no vendor service, network endpoint, or certificate is used.
//!
//! See `docs/evidence/re-update-install-20261105.md` for the captured schema,
//! row counts, and decoded timestamps.

/// Ledger rows are tiny; a bound keeps a corrupt ledger from being read whole.
#[cfg(windows)]
const MAX_HISTORY_ROWS: usize = 4096;
#[cfg(windows)]
const LEDGER_DIR: &str = r"Lenovo\Vantage\AddinData\LenovoSystemUpdateAddin\session";
#[cfg(windows)]
const INSTALL_EVENTS_DB: &str = "editable_update_history.db";
#[cfg(windows)]
const PACKAGE_STATUS_DB: &str = "update_history.db";

#[cfg(any(windows, test))]
mod ledger {
    use lctrl_core::{
        UpdateHistoryLedger, UpdateHistoryRecord, UpdateHistoryReport,
    };

    /// `SQLITE_OK`.
    pub(crate) const SQLITE_OK: i32 = 0;
    /// `SQLITE_ROW`: one row is available.
    pub(crate) const SQLITE_ROW: i32 = 100;
    /// `SQLITE_DONE`: no more rows.
    pub(crate) const SQLITE_DONE: i32 = 101;
    /// `SQLITE_OPEN_READONLY`.
    pub(crate) const SQLITE_OPEN_READONLY: i32 = 0x0000_0001;

    /// The install-event ledger, newest first. `utcinstalldate` is text, so the
    /// order is numeric, not lexicographic.
    pub(crate) const INSTALL_EVENTS_SQL: &str = "SELECT id, packagename, version, \
         utcinstalldate, status, packagesize, packagereleasedate, reboottype, crc \
         FROM updatehistory ORDER BY CAST(utcinstalldate AS INTEGER) DESC, id";

    /// The catalogue-status ledger. It has no timestamps to sort on.
    pub(crate) const PACKAGE_STATUS_SQL: &str = "SELECT id, title, version, status, \
         installdate, severity FROM updatehistory ORDER BY title, id";

    /// Trim a ledger cell; empty text means "the column is not populated".
    fn cell(row: &[Option<String>], index: usize) -> Option<String> {
        let value = row.get(index)?.as_ref()?.trim();
        (!value.is_empty()).then(|| value.to_string())
    }

    fn number(row: &[Option<String>], index: usize) -> Option<u64> {
        cell(row, index)?.parse().ok()
    }

    /// 100 ns ticks since 0001-01-01 → RFC 3339 UTC at the ledger's own 100 ns
    /// precision. An empty, zero, or unparseable value stays `None`.
    pub(crate) fn ticks_to_rfc3339(text: &str) -> Option<String> {
        let ticks: u64 = text.trim().parse().ok()?;
        if ticks == 0 {
            return None;
        }
        let seconds = ticks / 10_000_000;
        let fraction = ticks % 10_000_000;
        // Days from 0001-01-01 to 1970-01-01 (the civil-calendar epoch).
        const DAYS_0001_TO_1970: i64 = 719_162;
        let days = (seconds / 86_400) as i64 - DAYS_0001_TO_1970;
        let (year, month, day) = civil_from_days(days);
        let rem = seconds % 86_400;
        Some(format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{fraction:07}Z",
            rem / 3600,
            (rem % 3600) / 60,
            rem % 60
        ))
    }

    /// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → proleptic
    /// Gregorian date, without a calendar dependency.
    fn civil_from_days(days: i64) -> (i64, u32, u32) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let year = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if month <= 2 { year + 1 } else { year }, month, day)
    }

    /// Map one `editable_update_history.db` row. A row without an identifier or
    /// a status cannot be attributed to a package, so it is skipped.
    pub(crate) fn install_event(row: &[Option<String>]) -> Option<UpdateHistoryRecord> {
        Some(UpdateHistoryRecord {
            id: cell(row, 0)?,
            name: cell(row, 1),
            version: cell(row, 2),
            status: cell(row, 4)?,
            installed_at: cell(row, 3).as_deref().and_then(ticks_to_rfc3339),
            released_at: cell(row, 6),
            severity: None,
            reboot: cell(row, 7),
            size_bytes: number(row, 5),
            crc: cell(row, 8),
        })
    }

    /// Map one `update_history.db` row.
    pub(crate) fn package_status(row: &[Option<String>]) -> Option<UpdateHistoryRecord> {
        Some(UpdateHistoryRecord {
            id: cell(row, 0)?,
            name: cell(row, 1),
            version: cell(row, 2),
            status: cell(row, 3)?,
            installed_at: cell(row, 4).as_deref().and_then(ticks_to_rfc3339),
            released_at: None,
            severity: cell(row, 5),
            reboot: None,
            size_bytes: None,
            crc: None,
        })
    }

    pub(crate) fn report(
        ledger: UpdateHistoryLedger,
        source: String,
        rows: Vec<Vec<Option<String>>>,
    ) -> UpdateHistoryReport {
        let entries = match ledger {
            UpdateHistoryLedger::InstallEvents => {
                rows.iter().filter_map(|row| install_event(row)).collect()
            }
            UpdateHistoryLedger::PackageStatus => {
                rows.iter().filter_map(|row| package_status(row)).collect()
            }
        };
        UpdateHistoryReport {
            ledger,
            source,
            entries,
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) use ledger::*;

#[cfg(windows)]
mod imp {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::path::{Path, PathBuf};

    use lctrl_core::{LctrlError, Result, UpdateHistoryLedger, UpdateHistoryReport};
    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    use super::ledger::{
        INSTALL_EVENTS_SQL, PACKAGE_STATUS_SQL, SQLITE_DONE, SQLITE_OK, SQLITE_OPEN_READONLY,
        SQLITE_ROW, report,
    };
    use super::{INSTALL_EVENTS_DB, LEDGER_DIR, MAX_HISTORY_ROWS, PACKAGE_STATUS_DB};

    type SqliteHandle = *mut c_void;

    type OpenV2 =
        unsafe extern "system" fn(*const c_char, *mut SqliteHandle, c_int, *const c_char) -> c_int;
    type PrepareV2 = unsafe extern "system" fn(
        SqliteHandle,
        *const c_char,
        c_int,
        *mut SqliteHandle,
        *mut *const c_char,
    ) -> c_int;
    type Step = unsafe extern "system" fn(SqliteHandle) -> c_int;
    type ColumnCount = unsafe extern "system" fn(SqliteHandle) -> c_int;
    type ColumnText = unsafe extern "system" fn(SqliteHandle, c_int) -> *const u8;
    type ColumnBytes = unsafe extern "system" fn(SqliteHandle, c_int) -> c_int;
    type Finalize = unsafe extern "system" fn(SqliteHandle) -> c_int;
    type Close = unsafe extern "system" fn(SqliteHandle) -> c_int;
    type ErrMsg = unsafe extern "system" fn(SqliteHandle) -> *const c_char;

    fn unavailable(detail: String) -> LctrlError {
        LctrlError::ChannelUnavailable {
            channel: format!("Lenovo update ledger: {detail}"),
        }
    }

    /// One loaded `winsqlite3.dll`. Linking it at build time would make the
    /// crate unbuildable on images without it, and this crate also builds for
    /// Linux, where the channel is reported unavailable instead.
    struct Library(HMODULE);

    impl Drop for Library {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    FreeLibrary(self.0);
                }
            }
        }
    }

    impl Library {
        fn load() -> Result<Self> {
            let name: Vec<u16> = "winsqlite3.dll"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let module = unsafe { LoadLibraryW(name.as_ptr()) };
            if module.is_null() {
                return Err(unavailable(
                    "winsqlite3.dll is unavailable; the update ledger cannot be read on this image"
                        .into(),
                ));
            }
            Ok(Self(module))
        }

        /// Resolve one export. Every symbol below is SQLite's stable C API.
        unsafe fn symbol<T: Copy>(&self, name: &'static str) -> Result<T> {
            let text = CString::new(name)
                .map_err(|_| unavailable(format!("sqlite symbol {name:?} is not a C string")))?;
            let proc = unsafe { GetProcAddress(self.0, text.as_ptr().cast::<u8>()) }
                .ok_or_else(|| unavailable(format!("winsqlite3.dll does not export {name:?}")))?;
            Ok(unsafe { std::mem::transmute_copy(&proc) })
        }
    }

    /// The engine's function table. Holding the library keeps the pointers
    /// valid for the lifetime of the engine.
    struct Engine {
        _library: Library,
        open_v2: OpenV2,
        prepare_v2: PrepareV2,
        step: Step,
        column_count: ColumnCount,
        column_text: ColumnText,
        column_bytes: ColumnBytes,
        finalize: Finalize,
        close: Close,
        errmsg: ErrMsg,
    }

    impl Engine {
        fn load() -> Result<Self> {
            let library = Library::load()?;
            let open_v2 = unsafe { library.symbol("sqlite3_open_v2")? };
            let prepare_v2 = unsafe { library.symbol("sqlite3_prepare_v2")? };
            let step = unsafe { library.symbol("sqlite3_step")? };
            let column_count = unsafe { library.symbol("sqlite3_column_count")? };
            let column_text = unsafe { library.symbol("sqlite3_column_text")? };
            let column_bytes = unsafe { library.symbol("sqlite3_column_bytes")? };
            let finalize = unsafe { library.symbol("sqlite3_finalize")? };
            let close = unsafe { library.symbol("sqlite3_close")? };
            let errmsg = unsafe { library.symbol("sqlite3_errmsg")? };
            Ok(Self {
                _library: library,
                open_v2,
                prepare_v2,
                step,
                column_count,
                column_text,
                column_bytes,
                finalize,
                close,
                errmsg,
            })
        }

        fn error_text(&self, db: SqliteHandle, context: &str, code: c_int) -> LctrlError {
            let message = if db.is_null() {
                String::new()
            } else {
                let raw = unsafe { (self.errmsg)(db) };
                if raw.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(raw) }
                        .to_string_lossy()
                        .into_owned()
                }
            };
            unavailable(format!("{context} failed (sqlite code {code}): {message}"))
        }

        /// Read every row of one query as text columns, `None` for SQL NULL.
        /// The connection is opened `READONLY` and closed before returning.
        fn rows(&self, path: &Path, sql: &str) -> Result<Vec<Vec<Option<String>>>> {
            // `winsqlite3` takes the file name as UTF-8.
            let file = CString::new(path.to_string_lossy().as_bytes())
                .map_err(|_| unavailable("ledger path contains an interior NUL".into()))?;
            let mut db: SqliteHandle = std::ptr::null_mut();
            let status = unsafe {
                (self.open_v2)(
                    file.as_ptr(),
                    &mut db,
                    SQLITE_OPEN_READONLY,
                    std::ptr::null(),
                )
            };
            if status != SQLITE_OK || db.is_null() {
                let error = self.error_text(db, &format!("opening {path:?}"), status);
                if !db.is_null() {
                    unsafe {
                        (self.close)(db);
                    }
                }
                return Err(error);
            }
            let result = self.query(db, path, sql);
            unsafe {
                (self.close)(db);
            }
            result
        }

        fn query(
            &self,
            db: SqliteHandle,
            path: &Path,
            sql: &str,
        ) -> Result<Vec<Vec<Option<String>>>> {
            let statement = CString::new(sql)
                .map_err(|_| unavailable("ledger query contains an interior NUL".into()))?;
            let mut stmt: SqliteHandle = std::ptr::null_mut();
            let status = unsafe {
                (self.prepare_v2)(db, statement.as_ptr(), -1, &mut stmt, std::ptr::null_mut())
            };
            if status != SQLITE_OK || stmt.is_null() {
                let error = self.error_text(db, &format!("preparing a query on {path:?}"), status);
                if !stmt.is_null() {
                    unsafe {
                        (self.finalize)(stmt);
                    }
                }
                return Err(error);
            }
            let mut rows = Vec::new();
            loop {
                let status = unsafe { (self.step)(stmt) };
                if status == SQLITE_DONE {
                    break;
                }
                if status != SQLITE_ROW {
                    let error = self.error_text(db, &format!("reading {path:?}"), status);
                    unsafe {
                        (self.finalize)(stmt);
                    }
                    return Err(error);
                }
                if rows.len() >= MAX_HISTORY_ROWS {
                    unsafe {
                        (self.finalize)(stmt);
                    }
                    return Err(unavailable(format!(
                        "{path:?} holds more than {MAX_HISTORY_ROWS} rows"
                    )));
                }
                let columns = unsafe { (self.column_count)(stmt) }.max(0);
                let mut row = Vec::with_capacity(columns as usize);
                for index in 0..columns {
                    let raw = unsafe { (self.column_text)(stmt, index) };
                    if raw.is_null() {
                        row.push(None);
                        continue;
                    }
                    let length = unsafe { (self.column_bytes)(stmt, index) }.max(0) as usize;
                    let bytes = unsafe { std::slice::from_raw_parts(raw, length) };
                    row.push(Some(String::from_utf8_lossy(bytes).into_owned()));
                }
                rows.push(row);
            }
            unsafe {
                (self.finalize)(stmt);
            }
            Ok(rows)
        }
    }

    fn ledger_dir() -> Result<PathBuf> {
        let root = std::env::var_os("ProgramData").ok_or_else(|| {
            unavailable(
                "ProgramData is not set, so the update add-in session directory is unknown".into(),
            )
        })?;
        Ok(PathBuf::from(root).join(LEDGER_DIR))
    }

    /// Read the machine's update history: the install-event ledger when the
    /// add-in wrote one, otherwise its package-status ledger.
    pub fn read_history() -> Result<UpdateHistoryReport> {
        let directory = ledger_dir()?;
        let install_events = directory.join(INSTALL_EVENTS_DB);
        let package_status = directory.join(PACKAGE_STATUS_DB);
        let (path, ledger, sql) = if install_events.is_file() {
            (
                install_events,
                UpdateHistoryLedger::InstallEvents,
                INSTALL_EVENTS_SQL,
            )
        } else if package_status.is_file() {
            (
                package_status,
                UpdateHistoryLedger::PackageStatus,
                PACKAGE_STATUS_SQL,
            )
        } else {
            return Err(unavailable(format!(
                "neither {install_events:?} nor {package_status:?} exists; the Lenovo System Update add-in has not recorded anything here"
            )));
        };
        let engine = Engine::load()?;
        let rows = engine.rows(&path, sql)?;
        Ok(report(ledger, path.to_string_lossy().into_owned(), rows))
    }
}

#[cfg(windows)]
pub use imp::read_history;

/// Non-Windows build: the Lenovo add-in ledger only exists on Windows.
#[cfg(not(windows))]
pub fn read_history() -> lctrl_core::Result<lctrl_core::UpdateHistoryReport> {
    Err(lctrl_core::LctrlError::Unsupported {
        feature: "update.history.ledger".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_timestamp_is_net_ticks() {
        // Captured on the 21VG target: BIOS SWCN22WW, ledger row `swcn22ww`.
        assert_eq!(
            ticks_to_rfc3339("639237257469503680").as_deref(),
            Some("2026-08-30T22:29:06.9503680Z")
        );
        // An unpopulated or unparseable cell never becomes a timestamp.
        assert_eq!(ticks_to_rfc3339(""), None);
        assert_eq!(ticks_to_rfc3339("0"), None);
        assert_eq!(ticks_to_rfc3339("not-a-tick"), None);
    }

    #[test]
    fn install_event_row_maps_ledger_columns() {
        let row: Vec<Option<String>> = [
            "udc26605wx64",
            "UDC_266",
            "26.6.0.5",
            "639207511543692016",
            "Installed",
            "24501360",
            "2026-07-16",
            "NoRebootRequired",
            "34d30927f58b156c227358ebea88b30f61f7f9671d2539f57062342d14003a33",
        ]
        .into_iter()
        .map(|value| Some(value.to_string()))
        .collect();
        let record = install_event(&row).expect("row is attributable");
        assert_eq!(record.id, "udc26605wx64");
        assert_eq!(record.name.as_deref(), Some("UDC_266"));
        assert_eq!(record.version.as_deref(), Some("26.6.0.5"));
        assert_eq!(record.status, "Installed");
        assert_eq!(
            record.installed_at.as_deref(),
            Some("2026-07-27T12:12:34.3692016Z")
        );
        assert_eq!(record.released_at.as_deref(), Some("2026-07-16"));
        assert_eq!(record.reboot.as_deref(), Some("NoRebootRequired"));
        assert_eq!(record.size_bytes, Some(24_501_360));
        assert_eq!(record.crc.as_deref().map(str::len), Some(64));
        assert_eq!(record.severity, None);
    }

    #[test]
    fn unattributable_rows_are_skipped() {
        let blank: Vec<Option<String>> = vec![Some(String::new()), None, None, None, None, None];
        assert!(package_status(&blank).is_none());
        // The status ledger records no install timestamp.
        let row: Vec<Option<String>> = [
            "swcn22ww",
            "BIOS_SWCN1_22000",
            "SWCN22WW",
            "AlreadyInstalled",
            "",
            "Recommended",
        ]
        .into_iter()
        .map(|value| Some(value.to_string()))
        .collect();
        let record = package_status(&row).expect("row is attributable");
        assert_eq!(record.installed_at, None);
        assert_eq!(record.severity.as_deref(), Some("Recommended"));
    }
}

use std::cell::{Cell, RefCell};

use lctrl_core::{ApplyMode, LctrlError};
use lctrl_hal::{Setting, apply_setting};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    None,
    ReadAt(usize),
    Write,
}

struct InMemorySetting {
    value: RefCell<String>,
    readback_override: Option<String>,
    failure: Failure,
    reads: Cell<usize>,
    writes: Cell<usize>,
    calls: RefCell<Vec<&'static str>>,
}

impl InMemorySetting {
    fn new(value: &str) -> Self {
        Self {
            value: RefCell::new(value.into()),
            readback_override: None,
            failure: Failure::None,
            reads: Cell::new(0),
            writes: Cell::new(0),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn with_failure(mut self, failure: Failure) -> Self {
        self.failure = failure;
        self
    }

    fn with_readback(mut self, value: &str) -> Self {
        self.readback_override = Some(value.into());
        self
    }
}

impl Setting<String> for InMemorySetting {
    fn read(&self) -> lctrl_core::Result<String> {
        self.calls.borrow_mut().push("read");
        let read_number = self.reads.get() + 1;
        self.reads.set(read_number);
        if self.failure == Failure::ReadAt(read_number) {
            return Err(LctrlError::ChannelUnavailable {
                channel: "test-setting".into(),
            });
        }
        if read_number > 1 {
            if let Some(value) = &self.readback_override {
                return Ok(value.clone());
            }
        }
        Ok(self.value.borrow().clone())
    }

    fn write(&self, value: &String) -> lctrl_core::Result<()> {
        self.calls.borrow_mut().push("write");
        self.writes.set(self.writes.get() + 1);
        if self.failure == Failure::Write {
            return Err(LctrlError::FirmwareRejected {
                detail: "test write rejected".into(),
            });
        }
        self.value.replace(value.clone());
        Ok(())
    }
}

#[test]
fn dry_run_reads_once_without_writing_or_readback() {
    let setting = InMemorySetting::new("normal");

    let report = apply_setting(&setting, "conservation".to_string(), ApplyMode::DryRun)
        .expect("dry-run succeeds");

    assert_eq!(&*setting.calls.borrow(), &["read"]);
    assert_eq!(setting.reads.get(), 1);
    assert_eq!(setting.writes.get(), 0);
    assert_eq!(&*setting.value.borrow(), "normal");
    assert_eq!(report.previous(), "normal");
    assert_eq!(report.requested(), "conservation");
    assert_eq!(report.actual(), None);
}

#[test]
fn commit_reads_writes_and_reads_back_in_order() {
    let setting = InMemorySetting::new("normal");

    let report = apply_setting(&setting, "conservation".to_string(), ApplyMode::Commit)
        .expect("commit succeeds");

    assert_eq!(&*setting.calls.borrow(), &["read", "write", "read"]);
    assert_eq!(setting.reads.get(), 2);
    assert_eq!(setting.writes.get(), 1);
    assert_eq!(&*setting.value.borrow(), "conservation");
    assert_eq!(report.previous(), "normal");
    assert_eq!(report.requested(), "conservation");
    assert_eq!(report.actual().map(String::as_str), Some("conservation"));
}

#[test]
fn readback_mismatch_reports_requested_and_actual_values() {
    let setting = InMemorySetting::new("normal").with_readback("cool");

    let error = apply_setting(&setting, "performance".to_string(), ApplyMode::Commit)
        .expect_err("mismatch must fail");

    assert!(matches!(
        error,
        LctrlError::VerifyMismatch { requested, actual }
            if requested == "performance" && actual == "cool"
    ));
    assert_eq!(&*setting.calls.borrow(), &["read", "write", "read"]);
}

#[test]
fn write_error_is_returned_without_readback() {
    let setting = InMemorySetting::new("normal").with_failure(Failure::Write);

    let error = apply_setting(&setting, "conservation".to_string(), ApplyMode::Commit)
        .expect_err("write error must propagate");

    assert!(matches!(
        error,
        LctrlError::FirmwareRejected { detail } if detail == "test write rejected"
    ));
    assert_eq!(&*setting.calls.borrow(), &["read", "write"]);
    assert_eq!(setting.reads.get(), 1);
    assert_eq!(setting.writes.get(), 1);
}

#[test]
fn initial_read_error_suppresses_write() {
    let setting = InMemorySetting::new("normal").with_failure(Failure::ReadAt(1));

    let error = apply_setting(&setting, "conservation".to_string(), ApplyMode::Commit)
        .expect_err("read error must propagate");

    assert!(matches!(
        error,
        LctrlError::ChannelUnavailable { channel } if channel == "test-setting"
    ));
    assert_eq!(&*setting.calls.borrow(), &["read"]);
    assert_eq!(setting.writes.get(), 0);
}

#[test]
fn readback_error_propagates_after_one_write() {
    let setting = InMemorySetting::new("normal").with_failure(Failure::ReadAt(2));

    let error = apply_setting(&setting, "conservation".to_string(), ApplyMode::Commit)
        .expect_err("readback error must propagate");

    assert!(matches!(
        error,
        LctrlError::ChannelUnavailable { channel } if channel == "test-setting"
    ));
    assert_eq!(&*setting.calls.borrow(), &["read", "write", "read"]);
    assert_eq!(setting.writes.get(), 1);
}

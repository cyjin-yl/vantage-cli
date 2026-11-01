#![cfg(windows)]
// Windows-only integration test: the HAL only exposes this API on Windows.
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use lctrl_core::{
    ApplyMode, DriverIdentity, DriverPackage, DriverSignature, DriverUpdateReport, DriverVersion,
    IgnoredDriverRule, InstalledDriver, LctrlError, LocalDriverRollbackRequest,
    LocalDriverUpdateRequest, LocalUpdatePolicyStore, PresentPnpDevice, SignedDriverFile,
    UpdateSchedule, UpdateWeekday, validate_local_path,
};
use lctrl_hal::LocalUpdateControl;
use lctrl_hal_win::WindowsLocalUpdateController;
use parking_lot::Mutex;

const CLASS: &str = "{4d36e972-e325-11ce-bfc1-08002be10318}";
const FIRMWARE: &str = lctrl_core::FIRMWARE_DEVICE_CLASS_GUID;

/// The validator applies host path semantics, so fixtures have to be absolute
/// on the host that runs the suite.
fn host_abs(relative: &str) -> PathBuf {
    let root = if cfg!(windows) {
        PathBuf::from(r"C:\vantage-fixtures")
    } else {
        PathBuf::from("/vantage-fixtures")
    };
    root.join(relative)
}

fn package_inf() -> PathBuf {
    host_abs("packages/driver.inf")
}

fn identity(version: u16, section: &str) -> DriverIdentity {
    DriverIdentity {
        provider: "Signed Vendor".into(),
        description: "Test Network Driver".into(),
        class_guid: CLASS.into(),
        driver_version: DriverVersion::new([1, 0, version, 0]),
        driver_date: "10-24-2026".into(),
        matching_device_id: "USB\\VID_1234&PID_5678".into(),
        inf_section: section.into(),
    }
}

fn device() -> PresentPnpDevice {
    PresentPnpDevice {
        device_instance_id: "USB\\VID_1234&PID_5678\\01".into(),
        hardware_ids: vec!["USB\\VID_1234&PID_5678".into(), "USB\\VID_1234".into()],
        class_guid: CLASS.into(),
    }
}

fn signature() -> DriverSignature {
    DriverSignature {
        signer: "Test Driver Publishing CA".into(),
        signer_version: "2.0".into(),
    }
}

fn installed(version: u16, section: &str) -> InstalledDriver {
    InstalledDriver {
        identity: identity(version, section),
        published_inf: "oem12.inf".into(),
        driver_key: format!("{{{CLASS}}}\\0003"),
    }
}

fn package(inf: &Path) -> DriverPackage {
    DriverPackage {
        inf_path: inf.to_path_buf(),
        catalog_path: inf.parent().unwrap().join("package.cat"),
        signature: signature(),
        dependencies: vec![SignedDriverFile {
            path: inf.parent().unwrap().join("driver.sys"),
            catalog_path: inf.parent().unwrap().join("package.cat"),
            signature: signature(),
        }],
        identity: identity(3, "VendorInstall"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    Admit,
    Devices,
    Inspect,
    Verify,
    Read,
    Install,
    Rollback,
}

#[derive(Default)]
struct FakeApi {
    state: Mutex<FakeState>,
    calls: Mutex<Vec<Call>>,
}

#[derive(Default)]
struct FakeState {
    devices: Vec<PresentPnpDevice>,
    package: Option<DriverPackage>,
    trust_error: bool,
    trust_signer: Option<String>,
    current: Option<InstalledDriver>,
    after_install: Option<InstalledDriver>,
    after_rollback: Option<InstalledDriver>,
    install_done: bool,
    rollback_done: bool,
    reboot_required: bool,
    install_error: Option<LctrlError>,
    rollback_error: Option<LctrlError>,
    inf_override: Option<PathBuf>,
}

impl FakeApi {
    fn valid(inf: &Path) -> Self {
        Self {
            state: Mutex::new(FakeState {
                devices: vec![device()],
                package: Some(package(inf)),
                current: Some(installed(2, "OldInstall")),
                after_install: Some(installed(3, "VendorInstall")),
                ..FakeState::default()
            }),
            ..FakeApi::default()
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().clone()
    }

    fn record(&self, call: Call) {
        self.calls.lock().push(call);
    }
}

impl lctrl_hal_win::LocalDriverUpdateApi for FakeApi {
    fn admit_local_inf(&self, path: &Path) -> lctrl_core::Result<PathBuf> {
        self.record(Call::Admit);
        let state = self.state.lock();
        Ok(state
            .inf_override
            .clone()
            .unwrap_or_else(|| path.to_path_buf()))
    }

    fn present_devices(&self) -> lctrl_core::Result<Vec<PresentPnpDevice>> {
        self.record(Call::Devices);
        Ok(self.state.lock().devices.clone())
    }

    fn inspect_package(
        &self,
        inf: &Path,
        _device: &PresentPnpDevice,
        _hardware_id: &str,
    ) -> lctrl_core::Result<DriverPackage> {
        self.record(Call::Inspect);
        self.state
            .lock()
            .package
            .clone()
            .ok_or_else(|| LctrlError::InvalidArgument {
                detail: "fake package absent".into(),
            })
            .map(|mut value| {
                value.inf_path = inf.to_path_buf();
                value
            })
    }

    fn verify_package_signature(
        &self,
        package: &DriverPackage,
    ) -> lctrl_core::Result<DriverSignature> {
        self.record(Call::Verify);
        let state = self.state.lock();
        if state.trust_error {
            return Err(LctrlError::InvalidArgument {
                detail: "package signature was rejected".into(),
            });
        }
        Ok(DriverSignature {
            signer: state
                .trust_signer
                .clone()
                .unwrap_or_else(|| package.signature.signer.clone()),
            signer_version: package.signature.signer_version.clone(),
        })
    }

    fn installed_driver(&self, _device: &str) -> lctrl_core::Result<Option<InstalledDriver>> {
        self.record(Call::Read);
        let state = self.state.lock();
        if state.install_done {
            if let Some(after) = &state.after_install {
                return Ok(Some(after.clone()));
            }
        }
        if state.rollback_done {
            if let Some(after) = &state.after_rollback {
                return Ok(Some(after.clone()));
            }
        }
        Ok(state.current.clone())
    }

    fn install_driver(&self, _inf: &Path, _hardware_id: &str) -> lctrl_core::Result<bool> {
        self.record(Call::Install);
        let mut state = self.state.lock();
        if let Some(error) = state.install_error.take() {
            return Err(error);
        }
        state.install_done = true;
        Ok(state.reboot_required)
    }

    fn rollback_driver(&self, _device: &str) -> lctrl_core::Result<bool> {
        self.record(Call::Rollback);
        let mut state = self.state.lock();
        if let Some(error) = state.rollback_error.take() {
            return Err(error);
        }
        state.rollback_done = true;
        Ok(state.reboot_required)
    }
}

fn update_request(inf: &Path) -> LocalDriverUpdateRequest {
    LocalDriverUpdateRequest::new(inf, "USB\\VID_1234&PID_5678").unwrap()
}

fn rollback_request(old: u16, new: u16) -> LocalDriverRollbackRequest {
    LocalDriverRollbackRequest::new(
        "USB\\VID_1234&PID_5678\\01",
        identity(new, "VendorInstall"),
        identity(old, "OldInstall"),
    )
    .unwrap()
}

fn rollback_request_ambiguous() -> LocalDriverRollbackRequest {
    LocalDriverRollbackRequest::new(
        "USB\\VID_1234&PID_5678\\01",
        identity(3, "VendorInstall"),
        identity(2, "VendorInstall"),
    )
    .unwrap()
}

#[test]
fn remote_relative_and_wrong_file_inputs_are_refused() {
    for path in [
        "driver.inf",
        "https://example.test/driver.inf",
        "\\\\server\\share\\driver.inf",
        "/tmp/package.exe",
        "/tmp/../package.inf",
    ] {
        assert!(LocalDriverUpdateRequest::new(path, "USB\\VID_1234").is_err());
    }
    assert!(lctrl_core::is_firmware_class(FIRMWARE));
}

#[test]
fn absent_ambiguous_and_firmware_devices_refuse_before_package_trust() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    api.state.lock().devices.clear();
    let controller = WindowsLocalUpdateController::new(api);
    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );

    let api = FakeApi::valid(&inf);
    let mut device = device();
    device.device_instance_id = "second".into();
    api.state.lock().devices.push(device);
    let controller = WindowsLocalUpdateController::new(api);
    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );

    let api = FakeApi::valid(&inf);
    api.state.lock().devices[0].class_guid = FIRMWARE.into();
    let controller = WindowsLocalUpdateController::new(api);
    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );
}

#[test]
fn mismatched_package_class_or_hardware_id_is_refused_without_install() {
    let inf = package_inf();
    let mut package = package(&inf);
    package.identity.matching_device_id = "USB\\OTHER".into();
    let api = FakeApi::valid(&inf);
    api.state.lock().package = Some(package);
    let controller = WindowsLocalUpdateController::new(api);

    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );
    assert_eq!(
        controller.api().calls(),
        vec![Call::Admit, Call::Devices, Call::Inspect]
    );
}

#[test]
fn unsigned_or_wrongly_signed_package_is_refused_before_install() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    api.state.lock().trust_error = true;
    let controller = WindowsLocalUpdateController::new(api);
    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );

    let api = FakeApi::valid(&inf);
    api.state.lock().trust_signer = Some("Different Publisher".into());
    let controller = WindowsLocalUpdateController::new(api);
    assert!(
        controller
            .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
            .is_err()
    );
    assert!(!controller.api().calls().contains(&Call::Install));
}

#[test]
fn dry_run_verifies_and_binds_without_install_or_reboot_claim() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    let controller = WindowsLocalUpdateController::new(api);

    let report = controller
        .apply_local_driver_update(&update_request(&inf), ApplyMode::DryRun)
        .unwrap();
    assert_eq!(report.mode, ApplyMode::DryRun);
    assert_eq!(
        report.requested.driver_version,
        DriverVersion::new([1, 0, 3, 0])
    );
    assert_eq!(
        report.previous.as_ref().unwrap().identity.driver_version,
        DriverVersion::new([1, 0, 2, 0])
    );
    assert!(report.actual.is_none());
    assert!(report.reboot_required.is_none());
    assert_eq!(
        controller.api().calls(),
        vec![
            Call::Admit,
            Call::Devices,
            Call::Inspect,
            Call::Verify,
            Call::Read
        ]
    );
}

#[test]
fn commit_installs_once_and_requires_exact_identity_readback() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    let controller = WindowsLocalUpdateController::new(api);

    let report = controller
        .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.mode, ApplyMode::Commit);
    assert_eq!(report.reboot_required, Some(false));
    assert_eq!(
        report.actual.as_ref().unwrap().identity.driver_version,
        DriverVersion::new([1, 0, 3, 0])
    );
    assert_eq!(
        controller.api().calls(),
        vec![
            Call::Admit,
            Call::Devices,
            Call::Inspect,
            Call::Verify,
            Call::Read,
            Call::Install,
            Call::Read
        ]
    );
}

#[test]
fn failed_install_readback_is_semantic_and_does_not_fake_success() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    {
        let mut state = api.state.lock();
        state.after_install = Some(installed(2, "OldInstall"));
    }
    let controller = WindowsLocalUpdateController::new(api);

    let error = controller
        .apply_local_driver_update(&update_request(&inf), ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
    assert_eq!(error.exit_code(), 7);
}

#[test]
fn previous_driver_store_restore_commits_once_and_verifies_restored_identity() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    {
        let mut state = api.state.lock();
        state.current = Some(installed(3, "VendorInstall"));
        state.after_install = None;
        state.after_rollback = Some(installed(2, "OldInstall"));
    }
    let controller = WindowsLocalUpdateController::new(api);

    let report = controller
        .rollback_local_driver(&rollback_request(2, 3), ApplyMode::Commit)
        .unwrap();
    assert_eq!(report.mode, ApplyMode::Commit);
    assert!(report.backup_identity_verified);
    assert_eq!(
        report.previous.identity.driver_version,
        DriverVersion::new([1, 0, 3, 0])
    );
    assert_eq!(
        report.actual.as_ref().unwrap().identity.driver_version,
        DriverVersion::new([1, 0, 2, 0])
    );
    assert_eq!(
        controller.api().calls(),
        vec![Call::Devices, Call::Read, Call::Rollback, Call::Read]
    );
}

#[test]
fn rollback_without_documented_previous_point_and_bad_readback_refuse() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    {
        let mut state = api.state.lock();
        state.current = Some(installed(3, "VendorInstall"));
        state.rollback_error = Some(LctrlError::Unsupported {
            feature: "update.local_driver.rollback.no_previous_driver_store_point".into(),
        });
    }
    let controller = WindowsLocalUpdateController::new(api);
    let error = controller
        .rollback_local_driver(&rollback_request(2, 3), ApplyMode::Commit)
        .unwrap_err();
    assert!(matches!(error, LctrlError::Unsupported { .. }));

    let api = FakeApi::valid(&inf);
    {
        let mut state = api.state.lock();
        state.current = Some(installed(3, "VendorInstall"));
        state.after_install = None;
        state.after_rollback = Some(installed(1, "OldInstall"));
    }
    let controller = WindowsLocalUpdateController::new(api);
    assert!(matches!(
        controller
            .rollback_local_driver(&rollback_request_ambiguous(), ApplyMode::Commit)
            .unwrap_err(),
        LctrlError::VerifyMismatch { .. }
    ));
}

#[test]
fn rollback_dry_run_never_claims_backup_identity() {
    let inf = package_inf();
    let api = FakeApi::valid(&inf);
    api.state.lock().current = Some(installed(3, "VendorInstall"));
    let controller = WindowsLocalUpdateController::new(api);

    let report = controller
        .rollback_local_driver(&rollback_request(2, 3), ApplyMode::DryRun)
        .unwrap();
    assert!(!report.backup_identity_verified);
    assert!(report.actual.is_none());
    assert!(!controller.api().calls().contains(&Call::Rollback));
}

fn temp_root(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = format!(
        "lctrl-update-{}-{}-{label}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let root = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn committed_report() -> DriverUpdateReport {
    let _inf = package_inf();
    DriverUpdateReport {
        mode: ApplyMode::Commit,
        device_instance_id: "USB\\VID_1234&PID_5678\\01".into(),
        previous: Some(installed(2, "OldInstall")),
        requested: identity(3, "VendorInstall"),
        actual: Some(installed(3, "VendorInstall")),
        signature: signature(),
        reboot_required: Some(false),
    }
}

#[test]
fn policy_store_writes_exact_owner_path_without_credentials() {
    let root = temp_root("exact");
    let path = root.join("owner.json");
    let store = LocalUpdatePolicyStore::new(&path).unwrap();
    store
        .set_schedule(UpdateSchedule::weekly(UpdateWeekday::Sunday, 3, 4).unwrap())
        .unwrap();
    store
        .ignore(
            IgnoredDriverRule::new(
                "USB\\VID_1234&PID_5678",
                Some("Signed Vendor".into()),
                Some(DriverVersion::new([1, 0, 3, 0])),
            )
            .unwrap(),
        )
        .unwrap();
    let entry = store
        .record_install(&committed_report(), "2026-10-26T12:00:00Z")
        .unwrap();
    assert_eq!(entry.id, 1);
    assert_eq!(
        store
            .latest_for_device("usb\\VID_1234&PID_5678\\01")
            .unwrap()
            .unwrap()
            .actual
            .driver_version,
        DriverVersion::new([1, 0, 3, 0])
    );

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("/packages"));
    assert!(!text.contains("driver.sys"));
    let children = std::fs::read_dir(&root).unwrap().count();
    assert_eq!(children, 1);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn policy_json_rejects_unknown_credential_fields_without_persisting_them() {
    let root = temp_root("credentials");
    let path = root.join("owner.json");
    std::fs::write(
        &path,
        r#"{
  "schema": 1,
  "unc_password": "hunter2",
  "proxy": "http://user:secret@host",
  "history": [],
  "ignored": [],
  "schedule": {"kind": "off"}
}"#,
    )
    .unwrap();
    let store = LocalUpdatePolicyStore::new(&path).unwrap();
    let error = store.load().unwrap_err();
    assert!(!error.to_string().contains("hunter2"));
    assert!(!error.to_string().contains("user:secret"));
    assert!(matches!(error, LctrlError::InvalidArgument { .. }));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("hunter2"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn policy_store_rejects_remote_paths_and_invalid_schedules() {
    assert!(LocalUpdatePolicyStore::new(host_abs("policy.json")).is_ok());
    assert!(LocalUpdatePolicyStore::new("policy.json").is_err());
    assert!(LocalUpdatePolicyStore::new("\\\\server\\share\\policy.json").is_err());
    assert!(UpdateSchedule::daily(24, 0).is_err());
    assert!(UpdateSchedule::weekly(UpdateWeekday::Monday, 0, 60).is_err());
}

#[test]
fn local_path_validation_requires_absolute_regular_extension_semantics() {
    assert!(validate_local_path(&package_inf(), "inf").is_ok());
    assert!(validate_local_path(&host_abs("packages/driver.INF"), "inf").is_ok());
    assert!(validate_local_path(Path::new("driver.inf"), "inf").is_err());
    assert!(validate_local_path(&host_abs("packages/driver.sys"), "inf").is_err());
    assert!(validate_local_path(&host_abs("packages/../driver.inf"), "inf").is_err());
    assert!(validate_local_path(Path::new(r"\\server\share\driver.inf"), "inf").is_err());
    assert!(validate_local_path(Path::new(r"\\.\PhysicalDrive0"), "inf").is_err());
    #[cfg(windows)]
    {
        assert!(validate_local_path(Path::new(r"\\?\C:\packages\driver.inf"), "inf").is_ok());
        assert!(validate_local_path(Path::new(r"C:\packages\driver.inf"), "inf").is_ok());
        assert!(validate_local_path(Path::new("/packages/driver.inf"), "inf").is_err());
    }
    #[cfg(unix)]
    {
        assert!(validate_local_path(Path::new("/packages/driver.inf"), "inf").is_ok());
        assert!(validate_local_path(Path::new(r"C:\packages\driver.inf"), "inf").is_err());
        assert!(validate_local_path(Path::new(r"\\?\C:\packages\driver.inf"), "inf").is_err());
    }
}

#[test]
fn report_shape_has_no_path_or_credential_surface() {
    let report = committed_report();
    let value = serde_json::to_value(&report).unwrap();
    let object = value.as_object().unwrap();
    assert!(object.keys().all(|key| !key.contains("path")));
    assert_eq!(
        object.get("reboot_required"),
        Some(&serde_json::json!(false))
    );
}

#[allow(dead_code)]
fn ensure_send_sync() {
    use std::sync::Arc;
    fn bound<T: Send + Sync>(_: T) {}
    let inf = package_inf();
    bound(WindowsLocalUpdateController::new(FakeApi::valid(&inf)));
    bound(Arc::new(WindowsLocalUpdateController::new(FakeApi::valid(
        &inf,
    ))));
}

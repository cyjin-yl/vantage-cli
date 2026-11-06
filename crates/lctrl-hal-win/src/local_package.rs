//! Explicit, user-triggered installation of a Lenovo update package the UDC
//! service already downloaded.
//!
//! A UDC package is installed by the package's *own* installer script, not by
//! a command line invented here. The recovered contract on the 21VG target is
//! `%ProgramData%\Lenovo\Udc\Download\<package_id>\<version>\temp\install.ps1`,
//! a Lenovo-Authenticode-signed PowerShell script that:
//!
//! * exposes exactly one parameter, `-command install`;
//! * asserts elevation and exits `1` when it is not elevated;
//! * either pins its payload (`$Global:NewSetupFileName` +
//!   `$Global:setup_hash`, SHA-256 checked with `Get-FileHash`) or validates
//!   the payload itself;
//! * runs the vendor setup (`/VERYSILENT /AUTOSTART /run=ota`) and returns the
//!   setup's exit code;
//! * writes `%TEMP%\Lenovo<name>Log-<yyyy-MM-dd_HH-mm>.txt`.
//!
//! This module therefore only admits what it can prove: the directory must be
//! a UDC payload directory for a package the synced catalogue still lists, the
//! script's signature must be valid and issued to Lenovo, a hash-pinned payload
//! must match, and the process must be elevated. Firmware packages are refused
//! outright (charter §1 excludes firmware flashing) and nothing here reboots.
//!
//! See `docs/evidence/re-update-install-20261105.md`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use lctrl_core::{
    ApplyMode, LctrlError, LocalPackageInstallReport, LocalPackageInstallRequest,
    LocalPackagePayload, Result, is_firmware_update,
};
use lctrl_hal::LocalPackageInstallControl;

/// How long one package installer may run before the wait gives up. The
/// installer is *not* interrupted on expiry: killing a vendor setup midway
/// through is worse than reporting that it is still running.
#[cfg(windows)]
const INSTALL_BUDGET: std::time::Duration = std::time::Duration::from_secs(900);
/// Read-only helper invocations (signature, hash) are killed on expiry.
#[cfg(windows)]
const HELPER_BUDGET: std::time::Duration = std::time::Duration::from_secs(120);
#[cfg(windows)]
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
#[cfg(windows)]
const MAX_HELPER_OUTPUT: u64 = 8 * 1024;

/// The payload a package script hash-pins. Both fields are `None` when the
/// script does not declare the `$Global:NewSetupFileName`/`$Global:setup_hash`
/// pair; such a script validates its own payloads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptContract {
    pub setup_file: Option<String>,
    pub setup_sha256: Option<String>,
}

/// Parse the two globals a hash-pinned package script declares.
///
/// Anything else about the script is deliberately not interpreted: the script
/// is executed as the vendor wrote it, so only the declarations this backend
/// verifies are read.
pub fn parse_script_contract(text: &str) -> Result<ScriptContract> {
    let mut contract = ScriptContract::default();
    for line in text.lines() {
        if contract.setup_file.is_none() {
            if let Some(value) = assignment(line, "$Global:NewSetupFileName") {
                contract.setup_file = Some(value);
            }
        }
        if contract.setup_sha256.is_none() {
            if let Some(value) = assignment(line, "$Global:setup_hash") {
                contract.setup_sha256 = Some(value);
            }
        }
    }
    if let Some(file) = &contract.setup_file {
        validate_payload_name(file)?;
    }
    if let Some(hash) = &contract.setup_sha256 {
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(LctrlError::InvalidArgument {
                detail:
                    "package script declares a setup hash that is not 64 hexadecimal characters"
                        .into(),
            });
        }
        if contract.setup_file.is_none() {
            return Err(LctrlError::InvalidArgument {
                detail: "package script pins a payload hash without naming the payload file".into(),
            });
        }
    }
    Ok(contract)
}

/// One `$Global:name = "value"` assignment.
fn assignment(line: &str, name: &str) -> Option<String> {
    let rest = line.trim().strip_prefix(name)?;
    // `$Global:setup_hash_extra` is not `$Global:setup_hash`.
    if !rest.starts_with(|character: char| character.is_whitespace() || character == '=') {
        return None;
    }
    let (_, value) = rest.split_once('=')?;
    let value = value.trim().trim_end_matches(';').trim();
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    value[quote.len_utf8()..]
        .strip_suffix(quote)
        .map(str::to_string)
}

/// The declared payload must be a bare file name inside the package directory:
/// a package script may not point this backend at an arbitrary file.
fn validate_payload_name(file: &str) -> Result<()> {
    let path = Path::new(file);
    let bare = !file.is_empty()
        && !path.is_absolute()
        && path.components().count() == 1
        && !file.contains(['/', '\\'])
        && file != "."
        && file != "..";
    if !bare {
        return Err(LctrlError::InvalidArgument {
            detail: "package script payload name must be a bare file name".into(),
        });
    }
    Ok(())
}

/// Derive `(package_id, version)` from a UDC payload directory
/// `<download root>\<package_id>\<version>\temp`.
pub fn udc_package_from_dir(dir: &Path, download_root: &Path) -> Option<(String, String)> {
    let suffix = dir.strip_prefix(download_root).ok()?;
    let mut parts = suffix
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned());
    let package_id = parts.next()?;
    let version = parts.next()?;
    let leaf = parts.next()?;
    if parts.next().is_some()
        || !leaf.eq_ignore_ascii_case("temp")
        || package_id.is_empty()
        || version.is_empty()
    {
        return None;
    }
    Some((package_id, version))
}

/// What the synced UDC catalogue says about one package id.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageBinding {
    pub name: String,
    pub version: String,
    pub package_type: Option<String>,
}

/// A package directory that passed admission.
#[derive(Clone, Debug)]
pub struct AdmittedPackage {
    /// Canonical directory containing the installer script.
    pub dir: PathBuf,
    /// `<dir>\install.ps1`.
    pub script: PathBuf,
    /// The script text, decoded (UTF-8 lossy) and bounded by
    /// `MAX_LOCAL_PACKAGE_SCRIPT_BYTES`.
    pub script_text: String,
}

/// What the installer process returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunOutcome {
    /// Process exit code. `None` when the process ended without one.
    pub exit_code: Option<i32>,
}

/// The installer log this run produced, when the script wrote one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogFile {
    pub path: String,
}

/// Everything the controller needs from the platform. Keeping the mutating
/// call (`run_installer`) behind this trait means the admission and gate order
/// are testable without an installer or a signed package.
pub trait LocalPackageApi: Send + Sync {
    /// Canonical UDC download root (`%ProgramData%\Lenovo\Udc\Download`).
    fn download_root(&self) -> Result<PathBuf>;
    /// Canonicalize the directory and read its installer script.
    fn admit(&self, dir: &Path) -> Result<AdmittedPackage>;
    /// Verify the script's Authenticode signature; return the signer subject.
    fn verify_script(&self, script: &Path) -> Result<String>;
    /// SHA-256 of one payload file, lowercase hex.
    fn sha256_file(&self, file: &Path) -> Result<String>;
    /// The catalogue entry for `package_id`, when the synced config lists it.
    fn catalog_package(&self, package_id: &str) -> Result<Option<PackageBinding>>;
    /// Whether the current process holds an elevated administrator token.
    fn is_elevated(&self) -> Result<bool>;
    /// Run `install.ps1 -command install` and wait for it.
    fn run_installer(&self, script: &Path) -> Result<RunOutcome>;
    /// The newest installer log written at or after `since`.
    fn installer_log(&self, since: SystemTime) -> Result<Option<LogFile>>;
}

#[derive(Debug)]
pub struct WindowsLocalPackageInstaller<A> {
    api: A,
}

impl<A> WindowsLocalPackageInstaller<A> {
    #[must_use]
    pub const fn new(api: A) -> Self {
        Self { api }
    }

    #[must_use]
    pub const fn api(&self) -> &A {
        &self.api
    }
}

impl<A: LocalPackageApi> LocalPackageInstallControl for WindowsLocalPackageInstaller<A> {
    fn apply_local_package(
        &self,
        request: &LocalPackageInstallRequest,
        mode: ApplyMode,
    ) -> Result<LocalPackageInstallReport> {
        let admitted = self.api.admit(request.package_dir())?;
        let root = self.api.download_root()?;
        let (package_id, version) = udc_package_from_dir(&admitted.dir, &root).ok_or_else(|| {
            LctrlError::InvalidArgument {
                detail: format!(
                    "package directory must be the UDC payload directory <download root>\\<package_id>\\<version>\\temp, not {:?}",
                    admitted.dir
                ),
            }
        })?;
        let binding = self.api.catalog_package(&package_id)?.ok_or_else(|| {
            LctrlError::Unsupported {
                feature: format!("update.local-install.package.{package_id}"),
            }
        })?;
        if is_firmware_update(binding.package_type.as_deref(), &binding.name) {
            return Err(LctrlError::Unsupported {
                feature: format!("update.local-install.firmware.{package_id}"),
            });
        }
        let contract = parse_script_contract(&admitted.script_text)?;
        let signer = self.api.verify_script(&admitted.script)?;
        if !signer.to_ascii_uppercase().contains("LENOVO") {
            return Err(LctrlError::VerifyMismatch {
                requested: "an Authenticode signature issued to Lenovo".into(),
                actual: signer,
            });
        }
        let payload = match (&contract.setup_file, &contract.setup_sha256) {
            (Some(file), Some(expected)) => {
                let path = admitted.dir.join(file);
                let actual = self.api.sha256_file(&path)?;
                if !actual.eq_ignore_ascii_case(expected) {
                    return Err(LctrlError::VerifyMismatch {
                        requested: expected.clone(),
                        actual,
                    });
                }
                Some(LocalPackagePayload {
                    file: path.to_string_lossy().into_owned(),
                    sha256: actual.to_ascii_lowercase(),
                })
            }
            _ => None,
        };
        let script = admitted.script.to_string_lossy().into_owned();
        let mut report = LocalPackageInstallReport {
            mode,
            package_id,
            package_name: binding.name,
            package_version: version,
            package_type: binding.package_type,
            script,
            payload,
            signer,
            exit_code: None,
            log: None,
        };
        if mode == ApplyMode::DryRun {
            return Ok(report);
        }
        // The package script asserts elevation itself; checking first turns its
        // generic exit code 1 into a precise error.
        if !self.api.is_elevated()? {
            return Err(LctrlError::PermissionDenied {
                need: "an elevated administrator token; the package installer asserts elevation"
                    .into(),
            });
        }
        let started = SystemTime::now();
        let outcome = self.api.run_installer(&admitted.script)?;
        let log = self.api.installer_log(started)?.map(|log| log.path);
        let detail = log
            .as_ref()
            .map(|path| format!("; installer log: {path}"))
            .unwrap_or_default();
        match outcome.exit_code {
            Some(0) => {
                report.exit_code = Some(0);
                report.log = log;
                Ok(report)
            }
            Some(code) => Err(LctrlError::VerifyMismatch {
                requested: "package installer exit code 0".into(),
                actual: format!("exit code {code}{detail}"),
            }),
            None => Err(LctrlError::VerifyMismatch {
                requested: "package installer exit code 0".into(),
                actual: format!("installer ended without an exit code{detail}"),
            }),
        }
    }
}

#[cfg(windows)]
pub use native::NativeLocalPackageApi;

#[cfg(windows)]
mod native {
    use std::fs;
    use std::io::Read;
    use std::mem::size_of;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    use lctrl_core::{
        LctrlError, MAX_LOCAL_PACKAGE_SCRIPT_BYTES, Result, validate_local_path,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use super::{
        AdmittedPackage, HELPER_BUDGET, INSTALL_BUDGET, LocalPackageApi, LogFile,
        MAX_HELPER_OUTPUT, POLL_INTERVAL, PackageBinding, RunOutcome,
    };
    use crate::map_win_error;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const VERIFY_SCRIPT: &str = "$s = Get-AuthenticodeSignature -LiteralPath \
         $env:VANTAGE_PACKAGE_SCRIPT; [Console]::Out.WriteLine([string]$s.Status); \
         if ($s.SignerCertificate) { [Console]::Out.WriteLine($s.SignerCertificate.Subject) }";
    const HASH_FILE: &str = "$h = Get-FileHash -LiteralPath $env:VANTAGE_PACKAGE_FILE \
         -Algorithm SHA256; [Console]::Out.WriteLine($h.Hash)";
    const LOG_PREFIX: &str = "Lenovo";
    const LOG_SUFFIX: &str = ".txt";
    const LOG_MARKER: &str = "Log-";

    fn unavailable(detail: String) -> LctrlError {
        LctrlError::ChannelUnavailable {
            channel: format!("local package install: {detail}"),
        }
    }

    fn invalid(detail: impl Into<String>) -> LctrlError {
        LctrlError::InvalidArgument {
            detail: detail.into(),
        }
    }

    fn contextual_io(error: std::io::Error, context: &str) -> LctrlError {
        LctrlError::Io(std::io::Error::new(
            error.kind(),
            format!("{context}: {error}"),
        ))
    }

    /// Public Windows APIs and the package's own signed installer only. No
    /// catalogue download, no firmware path, no reboot, no service control.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct NativeLocalPackageApi;

    impl LocalPackageApi for NativeLocalPackageApi {
        fn download_root(&self) -> Result<PathBuf> {
            let root = std::env::var_os("ProgramData").ok_or_else(|| {
                unavailable(
                    "ProgramData is not set, so the UDC download root is unknown".into(),
                )
            })?;
            let root = PathBuf::from(root).join(r"Lenovo\Udc\Download");
            fs::canonicalize(&root).map_err(|error| {
                contextual_io(
                    error,
                    &format!("canonicalizing the UDC download root {root:?}"),
                )
            })
        }

        fn admit(&self, dir: &Path) -> Result<AdmittedPackage> {
            let dir = fs::canonicalize(dir)
                .map_err(|error| contextual_io(error, "canonicalizing the package directory"))?;
            let metadata = fs::metadata(&dir)
                .map_err(|error| contextual_io(error, "reading the package directory"))?;
            if !metadata.is_dir() {
                return Err(invalid("package path must be a directory"));
            }
            let script = dir.join("install.ps1");
            validate_local_path(&script, "ps1")?;
            let metadata = fs::metadata(&script)
                .map_err(|error| contextual_io(error, "reading the package installer script"))?;
            if !metadata.is_file() {
                return Err(invalid(
                    "package installer script is not a regular non-symlink file",
                ));
            }
            if metadata.len() > MAX_LOCAL_PACKAGE_SCRIPT_BYTES {
                return Err(invalid(format!(
                    "package installer script exceeds the {MAX_LOCAL_PACKAGE_SCRIPT_BYTES}-byte bound"
                )));
            }
            let mut bytes = Vec::new();
            fs::File::open(&script)
                .map_err(|error| contextual_io(error, "opening the package installer script"))?
                .take(MAX_LOCAL_PACKAGE_SCRIPT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| contextual_io(error, "reading the package installer script"))?;
            if bytes.len() as u64 > MAX_LOCAL_PACKAGE_SCRIPT_BYTES {
                return Err(invalid(
                    "package installer script grew past its size bound while being read",
                ));
            }
            let text = String::from_utf8_lossy(&bytes);
            Ok(AdmittedPackage {
                dir,
                script,
                script_text: text.trim_start_matches('\u{feff}').to_string(),
            })
        }

        fn verify_script(&self, script: &Path) -> Result<String> {
            let output = run_helper(
                &["-Command", VERIFY_SCRIPT],
                ("VANTAGE_PACKAGE_SCRIPT", script),
                HELPER_BUDGET,
            )?;
            let text = String::from_utf8_lossy(&output);
            let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
            let status = lines.next().unwrap_or_default();
            if !status.eq_ignore_ascii_case("Valid") {
                return Err(LctrlError::VerifyMismatch {
                    requested: "a valid Authenticode signature".into(),
                    actual: if status.is_empty() {
                        "the signature check reported no status".into()
                    } else {
                        status.to_string()
                    },
                });
            }
            let subject = lines.next().ok_or_else(|| {
                unavailable("the signature check reported Valid without a signer".into())
            })?;
            Ok(subject.to_string())
        }

        fn sha256_file(&self, file: &Path) -> Result<String> {
            let output = run_helper(
                &["-Command", HASH_FILE],
                ("VANTAGE_PACKAGE_FILE", file),
                HELPER_BUDGET,
            )?;
            let text = String::from_utf8_lossy(&output);
            let hash = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default();
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(unavailable(format!(
                    "hashing {file:?} did not produce a SHA-256 digest"
                )));
            }
            Ok(hash.to_ascii_lowercase())
        }

        fn catalog_package(&self, package_id: &str) -> Result<Option<PackageBinding>> {
            Ok(
                crate::update_catalog::package_binding(package_id)?.map(|package| {
                    PackageBinding {
                        name: package.name,
                        version: package.version,
                        package_type: package.package_type,
                    }
                }),
            )
        }

        fn is_elevated(&self) -> Result<bool> {
            let mut token: HANDLE = std::ptr::null_mut();
            let opened =
                unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
            if opened == 0 {
                return Err(map_win_error(
                    unsafe { GetLastError() },
                    "opening this process token",
                ));
            }
            let mut elevation = TOKEN_ELEVATION {
                TokenIsElevated: 0,
            };
            let mut length = 0u32;
            let queried = unsafe {
                GetTokenInformation(
                    token,
                    TokenElevation,
                    (&mut elevation as *mut TOKEN_ELEVATION).cast(),
                    size_of::<TOKEN_ELEVATION>() as u32,
                    &mut length,
                )
            };
            unsafe {
                CloseHandle(token);
            }
            if queried == 0 {
                return Err(map_win_error(
                    unsafe { GetLastError() },
                    "reading this process token elevation",
                ));
            }
            Ok(elevation.TokenIsElevated != 0)
        }

        fn run_installer(&self, script: &Path) -> Result<RunOutcome> {
            let mut command = Command::new("powershell.exe");
            command
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(script)
                .args(["-command", "install"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW);
            let mut child = command
                .spawn()
                .map_err(|error| contextual_io(error, "starting the package installer"))?;
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        return Ok(RunOutcome {
                            exit_code: status.code(),
                        });
                    }
                    Ok(None) => {}
                    Err(error) => {
                        return Err(contextual_io(
                            error,
                            "waiting for the package installer",
                        ));
                    }
                }
                if started.elapsed() >= INSTALL_BUDGET {
                    // Deliberately no kill: an interrupted vendor setup can
                    // leave a half-applied package. Report and let it finish.
                    return Err(unavailable(format!(
                        "the package installer is still running after {}s and was not interrupted; check its log before retrying",
                        INSTALL_BUDGET.as_secs()
                    )));
                }
                thread::sleep(POLL_INTERVAL);
            }
        }

        fn installer_log(&self, since: SystemTime) -> Result<Option<LogFile>> {
            let directory = std::env::temp_dir();
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(contextual_io(error, "reading the temp directory")),
            };
            // Filesystem timestamps are coarse; accept a small skew.
            let floor = since.checked_sub(Duration::from_secs(2));
            let mut newest: Option<(SystemTime, PathBuf)> = None;
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !(name.starts_with(LOG_PREFIX)
                    && name.ends_with(LOG_SUFFIX)
                    && name.contains(LOG_MARKER))
                {
                    continue;
                }
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let Ok(modified) = metadata.modified() else {
                    continue;
                };
                if floor.is_some_and(|floor| modified < floor) {
                    continue;
                }
                if newest
                    .as_ref()
                    .is_none_or(|(newest_at, _)| modified > *newest_at)
                {
                    newest = Some((modified, entry.path()));
                }
            }
            Ok(newest.map(|(_, path)| LogFile {
                path: path.to_string_lossy().into_owned(),
            }))
        }
    }

    /// Run one read-only PowerShell helper with a bounded wait and kill it on
    /// expiry. The output is one line, so draining after exit cannot deadlock.
    fn run_helper(
        args: &[&str],
        environment: (&str, &Path),
        budget: Duration,
    ) -> Result<Vec<u8>> {
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass"])
            .args(args)
            .env(environment.0, environment.1)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|error| contextual_io(error, "starting a read-only helper"))?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(contextual_io(error, "waiting for a read-only helper"));
                }
            }
            if started.elapsed() >= budget {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unavailable(format!(
                    "a read-only helper exceeded its {}-second bound",
                    budget.as_secs()
                )));
            }
            thread::sleep(POLL_INTERVAL);
        }
        let mut bytes = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout
                .take(MAX_HELPER_OUTPUT)
                .read_to_end(&mut bytes)
                .map_err(|error| contextual_io(error, "reading helper output"))?;
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_pinned_script_declares_its_payload() {
        let script = r#"
param(
    [ValidateSet("install")]
    [string]$command
)

$Global:NewSetupFileName = "MagiCenter_2.0.5.501_cid_UDC_build1438.exe"

$Global:setup_hash = "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435"
"#;
        let contract = parse_script_contract(script).unwrap();
        assert_eq!(
            contract.setup_file.as_deref(),
            Some("MagiCenter_2.0.5.501_cid_UDC_build1438.exe")
        );
        assert_eq!(
            contract.setup_sha256.as_deref(),
            Some("8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435")
        );
    }

    #[test]
    fn self_validating_script_has_no_pinned_payload() {
        // The Smart Meeting package validates its payloads inside the script
        // and declares no hash pair; that is an honest `None`, not a failure.
        let contract = parse_script_contract("$Global:InstallerName = Join-Path $PSScriptRoot 'x.exe'")
            .unwrap();
        assert_eq!(contract, ScriptContract::default());
    }

    #[test]
    fn malformed_or_pathlike_payloads_are_refused() {
        assert!(
            parse_script_contract("$Global:NewSetupFileName = \"..\\evil.exe\"").is_err(),
            "a relative traversal is not a bare file name"
        );
        assert!(parse_script_contract("$Global:NewSetupFileName = \"C:\\evil.exe\"").is_err());
        assert!(parse_script_contract("$Global:setup_hash = \"abc\"").is_err());
        assert!(
            parse_script_contract("$Global:setup_hash = \"8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435\"")
                .is_err(),
            "a pinned hash without a payload name cannot be verified"
        );
    }

    struct FakeApi {
        root: PathBuf,
        package_id: String,
        version: String,
        text: String,
        binding: Option<PackageBinding>,
        signer: String,
        payload_sha256: Option<String>,
        elevated: bool,
        exit_code: Option<i32>,
        calls: std::sync::Mutex<Vec<&'static str>>,
    }

    /// An absolute root in the host's own form, so the admission rules are the
    /// real ones on both the Linux test host and a Windows target.
    fn absolute_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\ProgramData\Lenovo\Udc\Download")
        } else {
            PathBuf::from("/download")
        }
    }

    impl FakeApi {
        fn new(text: &str) -> Self {
            Self {
                root: absolute_root(),
                package_id: "pkg-1".into(),
                version: "1.2.3".into(),
                text: text.into(),
                binding: Some(PackageBinding {
                    name: "Lenovo Example".into(),
                    version: "1.2.3".into(),
                    package_type: Some("app".into()),
                }),
                signer: "CN=LENOVO (UNITED STATES) INC., O=LENOVO (UNITED STATES) INC.".into(),
                payload_sha256: None,
                elevated: true,
                exit_code: Some(0),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn dir(&self) -> PathBuf {
            self.root.join(&self.package_id).join(&self.version).join("temp")
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl LocalPackageApi for FakeApi {
        fn download_root(&self) -> Result<PathBuf> {
            Ok(self.root.clone())
        }

        fn admit(&self, dir: &Path) -> Result<AdmittedPackage> {
            self.calls.lock().unwrap().push("admit");
            let dir = dir.to_path_buf();
            Ok(AdmittedPackage {
                script: dir.join("install.ps1"),
                dir,
                script_text: self.text.clone(),
            })
        }

        fn verify_script(&self, _script: &Path) -> Result<String> {
            self.calls.lock().unwrap().push("verify");
            Ok(self.signer.clone())
        }

        fn sha256_file(&self, _file: &Path) -> Result<String> {
            self.calls.lock().unwrap().push("hash");
            self.payload_sha256
                .clone()
                .ok_or_else(|| LctrlError::Io(std::io::Error::other("missing payload")))
        }

        fn catalog_package(&self, package_id: &str) -> Result<Option<PackageBinding>> {
            self.calls.lock().unwrap().push("catalog");
            assert_eq!(package_id, self.package_id);
            Ok(self.binding.clone())
        }

        fn is_elevated(&self) -> Result<bool> {
            self.calls.lock().unwrap().push("elevated");
            Ok(self.elevated)
        }

        fn run_installer(&self, _script: &Path) -> Result<RunOutcome> {
            self.calls.lock().unwrap().push("run");
            Ok(RunOutcome {
                exit_code: self.exit_code,
            })
        }

        fn installer_log(&self, _since: SystemTime) -> Result<Option<LogFile>> {
            Ok(Some(LogFile {
                path: "C:\\Temp\\LenovoExampleLog-2026-11-05_12-00.txt".into(),
            }))
        }
    }

    const PINNED: &str = r#"
$Global:NewSetupFileName = "Example_1.2.3.exe"
$Global:setup_hash = "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435"
"#;

    fn request(api: &FakeApi) -> LocalPackageInstallRequest {
        LocalPackageInstallRequest::new(api.dir()).unwrap()
    }

    #[test]
    fn dry_run_verifies_every_gate_and_starts_nothing() {
        let mut api = FakeApi::new(PINNED);
        api.payload_sha256 = Some(
            "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435".into(),
        );
        let installer = WindowsLocalPackageInstaller::new(api);
        let report = installer
            .apply_local_package(&request(&installer.api), ApplyMode::DryRun)
            .unwrap();
        assert_eq!(report.mode, ApplyMode::DryRun);
        assert_eq!(report.package_name, "Lenovo Example");
        assert_eq!(report.package_version, "1.2.3");
        assert!(report.signer.contains("LENOVO"));
        assert_eq!(
            report.payload.as_ref().map(|payload| payload.sha256.as_str()),
            Some("8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435")
        );
        assert_eq!(report.exit_code, None);
        assert!(!installer.api.calls().contains(&"run"));
        assert!(!installer.api.calls().contains(&"elevated"));
    }

    #[test]
    fn firmware_packages_are_refused_before_any_verification() {
        let mut api = FakeApi::new(PINNED);
        api.binding = Some(PackageBinding {
            name: "BIOS_SWCN1_22000".into(),
            version: "SWCN22WW".into(),
            package_type: Some("bios".into()),
        });
        let installer = WindowsLocalPackageInstaller::new(api);
        let error = installer
            .apply_local_package(&request(&installer.api), ApplyMode::Commit)
            .unwrap_err();
        assert!(matches!(error, LctrlError::Unsupported { .. }));
        let calls = installer.api.calls();
        assert!(calls.contains(&"catalog"));
        assert!(!calls.contains(&"verify"));
        assert!(!calls.contains(&"run"));
    }

    #[test]
    fn untrusted_signer_and_hash_mismatch_stop_before_running() {
        let mut api = FakeApi::new(PINNED);
        api.signer = "CN=Somebody Else".into();
        let installer = WindowsLocalPackageInstaller::new(api);
        assert!(matches!(
            installer
                .apply_local_package(&request(&installer.api), ApplyMode::Commit)
                .unwrap_err(),
            LctrlError::VerifyMismatch { .. }
        ));
        assert!(!installer.api.calls().contains(&"run"));

        let mut api = FakeApi::new(PINNED);
        api.payload_sha256 = Some("00".repeat(32));
        let installer = WindowsLocalPackageInstaller::new(api);
        assert!(matches!(
            installer
                .apply_local_package(&request(&installer.api), ApplyMode::Commit)
                .unwrap_err(),
            LctrlError::VerifyMismatch { .. }
        ));
        assert!(!installer.api.calls().contains(&"run"));
    }

    #[test]
    fn unelevated_commit_is_refused_but_dry_run_is_allowed() {
        let mut api = FakeApi::new(PINNED);
        api.payload_sha256 = Some(
            "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435".into(),
        );
        api.elevated = false;
        let installer = WindowsLocalPackageInstaller::new(api);
        assert!(
            installer
                .apply_local_package(&request(&installer.api), ApplyMode::DryRun)
                .is_ok()
        );
        assert!(matches!(
            installer
                .apply_local_package(&request(&installer.api), ApplyMode::Commit)
                .unwrap_err(),
            LctrlError::PermissionDenied { .. }
        ));
        assert!(!installer.api.calls().contains(&"run"));
    }

    #[test]
    fn installer_result_is_the_committed_outcome() {
        let mut api = FakeApi::new(PINNED);
        api.payload_sha256 = Some(
            "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435".into(),
        );
        let installer = WindowsLocalPackageInstaller::new(api);
        let report = installer
            .apply_local_package(&request(&installer.api), ApplyMode::Commit)
            .unwrap();
        assert_eq!(report.exit_code, Some(0));
        assert!(report.log.is_some());
        assert!(installer.api.calls().contains(&"run"));
    }

    #[test]
    fn non_zero_installer_exit_is_a_verify_mismatch_carrying_the_log() {
        let mut api = FakeApi::new(PINNED);
        api.payload_sha256 = Some(
            "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435".into(),
        );
        api.exit_code = Some(1);
        let installer = WindowsLocalPackageInstaller::new(api);
        let error = installer
            .apply_local_package(&request(&installer.api), ApplyMode::Commit)
            .unwrap_err();
        match error {
            LctrlError::VerifyMismatch { actual, .. } => {
                assert!(actual.contains("exit code 1"), "{actual}");
                assert!(actual.contains("installer log"), "{actual}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn package_identity_comes_from_the_udc_layout() {
        let root = Path::new(r"\\?\C:\ProgramData\Lenovo\Udc\Download");
        let dir = Path::new(
            r"\\?\C:\ProgramData\Lenovo\Udc\Download\6c28b858-18c6-49f3-9819-b002f41a1492\2.0.5.501\temp",
        );
        assert_eq!(
            udc_package_from_dir(dir, root),
            Some((
                "6c28b858-18c6-49f3-9819-b002f41a1492".to_string(),
                "2.0.5.501".to_string()
            ))
        );
        // Not a payload directory.
        assert_eq!(
            udc_package_from_dir(
                Path::new(
                    r"\\?\C:\ProgramData\Lenovo\Udc\Download\6c28b858-18c6-49f3-9819-b002f41a1492\2.0.5.501"
                ),
                root
            ),
            None
        );
        // Outside the download root.
        assert_eq!(
            udc_package_from_dir(Path::new(r"\\?\C:\Users\ezra\package\temp"), root),
            None
        );
    }
}

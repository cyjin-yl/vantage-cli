# Update install + history — recovered package→installer contract and install ledger

Scope: `UPDATE_INSTALL` / `UPDATE_ROLLBACK` / `UPDATE_HISTORY` / `UPDATE_IGNORE` /
`UPDATE_SCHEDULE` / `UPDATE_LOCAL_INSTALL` / `UPDATE_LOCAL_ROLLBACK`.

Question for this slice: the catalog *download* is mTLS-gated (see
`udc-mtls-credential-hunt-20261105.md`), but is a **local** package install
possible without mTLS, and where does the machine record what it installed?

Answer: yes for packages the UDC service already downloaded, and the ledger is
two SQLite databases under the Lenovo System Update add-in's session directory.
Both are now surfaced read-only / contract-only respectively.

All target evidence below was collected **read-only** from `tb14p-ezra`
(21VG, Windows 11 26200) over SSH on 2026-11-05: registry queries, directory
listings, file copies, and `strings`. Nothing was installed, started, written
back, or reconfigured on the target; the two package scripts and two ledgers
were copied off and inspected locally.

---

## 1. The installer command contract

The updater on this machine is Lenovo **UDC** ("Unified Device Catalog"). It
does not have a public installer CLI. Each package instead ships its **own
installer script**, which the UDC service stages under the download cache:

```
%ProgramData%\Lenovo\Udc\Download\<package_id>\<version>\temp\install.ps1
```

Live contents of that root on the target:

| Package id | Version | Payload directory | Files left behind |
|---|---|---|---|
| `6c28b858-18c6-49f3-9819-b002f41a1492` | `2.0.5.501` (LenovoMagiCenter) | `…\6c28b858…/2.0.5.501/temp` | `install.ps1` (19 266 B) |
| `a642ce0d-4c2e-41ba-8a19-de691e5c896a` | `4.2.0.0908` (联想智会 / Lenovo Smart Meeting) | `…\a642ce0d…/4.2.0.0908/temp` | `install.ps1` (41 144 B) |
| `1b4e28ba-2fa1-11d2-883f-0016d3cca427` | `1.3.0.6` (Lenovo Speech) | `…\1b4e28ba…/1.3.0.6` | `lip-plugin_manifest.json` |

The UDC service deletes the payload executables after a successful install;
only `install.ps1` remains. That is why the earlier recon
(`update-contract-recon-20261104.md`) could not see a handler: the
`package_handler` field of every entry in `LastUpdateConfig` is the empty
string, and the real contract is the per-package script, not a shared handler
name.

### 1.1 Invocation

Verbatim parameter block of `6c28b858…/2.0.5.501/temp/install.ps1`:

```powershell
param(
    [ValidateSet("install")]
    [string]$command
)
```

The only accepted argument is `-command install`; any other value exits `1`
with `"No valid argument provided. Exiting..."`. The service invokes it as a
PowerShell script file (not a function, not a dot-source), so the contract is:

```
powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass \
    -File <package_dir>\install.ps1 -command install
```

### 1.2 Elevation, hash gate, silent setup (same script, verbatim)

```powershell
$Global:NewSetupFileName = "MagiCenter_2.0.5.501_cid_UDC_build1438.exe"
$Global:setup_hash = "8b2b73b64fbe3f8f7e916a41000885e7eb68c249302075c1e40f9b863e96a435"
$Constants = @{
    PathToLogFile = "$( $env:TEMP )\LenovoMagiCenterLog-$( Get-Date -Format 'yyyy-MM-dd_HH-mm' ).txt"
    ErrorCodes    = @{ ScriptRequiresElevation = 1; ExecutableNotPresent = 2 }
}
…
function Assert-Elevation { … exit $Constants.ErrorCodes.ScriptRequiresElevation }
function Install-APP {
    $Global:InstallerName = Join-Path $PSScriptRoot $($Global:NewSetupFileName)
    $fileHash = Get-FileHash "$($Global:InstallerName)" -Algorithm SHA256
    if ($fileHash.Hash -ne $Global:setup_hash) { Write-Log "Verify setup exe hash failed"; return 1 }
    $arguments = @("/VERYSILENT", "/AUTOSTART", "/run=ota")
    $process = Start-Process -FilePath "$($Global:InstallerName)" -ArgumentList $arguments -PassThru
    $process | Wait-Process
    …
    return $process.ExitCode
}
```

Consequences that matter for a CLI:

* **Elevation is mandatory**; the script exits `1` when not elevated.
* **The payload is hash-pinned**: SHA-256 of `$Global:NewSetupFileName`, checked
  by the script itself with `Get-FileHash`.
* The setup is **silent** (`/VERYSILENT`), so no installer window is expected.
* The **process exit code is the result**; `0` means the setup succeeded.
* The script logs to `%TEMP%\Lenovo<name>Log-<yyyy-MM-dd_HH-mm>.txt`.

### 1.3 The script is Lenovo-signed (live, both packages)

`Get-AuthenticodeSignature` on both staged scripts:

```
Status:  Valid | Signature verified.
Subject: CN=LENOVO (UNITED STATES) INC., O=LENOVO (UNITED STATES) INC.,
         L=Morrisville, S=North Carolina, C=US, SERIALNUMBER=3912990, …
Issuer:  CN=DigiCert Trusted G4 Code Signing RSA4096 SHA384 2021 CA1,
         O="DigiCert, Inc.", C=US
NotAfter: 2027-07-24
```

Both scripts carry a full PKCS#7 `# SIG # Begin signature block`, so the
signature is a property of the file, not of the directory.

### 1.4 Script shapes differ per package — only the verified part is automated

The MagiCenter script declares the hash pair above. The Smart Meeting script
(`a642ce0d…`) does **not**: it hard-codes its installer names
(`Lenovo_Smart_Meeting_x64_stable4.2_4.2.0.0908_3360_cid_ota.exe`,
`LenovoSpeech_…_2.1.0.90.exe`) and validates payloads itself through
`Lenovo.CertificateValidation.Native.dll` / `ValidateAssembly`.

So the generic contract is `install.ps1 -command install` + a valid Lenovo
signature; the hash-pinned payload is an **optional extra gate** that exists in
some packages. `update local-install` therefore:

* requires the Lenovo signature (always);
* verifies the payload hash **when the script declares one**, and reports
  `payload: null` when it does not (no fabricated hash);
* never interprets the rest of the script.

---

## 2. The update history ledger

### 2.1 Location

```
%ProgramData%\Lenovo\Vantage\AddinData\LenovoSystemUpdateAddin\session\
    editable_update_history.db      # install events (real install timestamps)
    update_history.db               # per-package catalogue status
    UpdateTaskList.xml              # current session state
    UpdateSchedule.xml              # schedule request the add-in last wrote
    ProblematicUpdates.xml          # ignore/exclusion list (empty here)
    SessionRecord.xml, updates.json, available_updates.json, aggregated_device_updates.json
```

Both databases are ordinary SQLite 3 files (page size 4096, UTF-8, 3 pages,
read/write format version 1, no WAL sidecar at capture time):

| File | Size | SHA-256 (copied artifact) |
|---|---|---|
| `editable_update_history.db` | 12 288 B | `fac1e0d8bf960b896ad74a9e3eb3d7e465b75141809008645904dac73198d490` |
| `update_history.db` | 12 288 B | `cc55911926acccd166f5949d2aee502ae2e6e2a3fe64ec7c7f6f97998041777f` |

### 2.2 Schemas

```sql
-- editable_update_history.db
CREATE TABLE updatehistory (
    id text PRIMARY KEY, packagename text, packagedesc text, version text,
    utcinstalldate text, status text, packagesize text, packagereleasedate text,
    reboottype text, crc text, hardwareid text
);

-- update_history.db
CREATE TABLE updatehistory (
    id text PRIMARY KEY, title text, status text, version text,
    additionalinfo text, severity text, installdate text, hardwareid text
);
```

### 2.3 Rows (live, 4 + 30)

`editable_update_history.db` — every recorded install, decoded:

| id | name | version | `utcinstalldate` (ticks) | UTC install time | status | reboot | size | crc |
|---|---|---|---|---|---|---|---|---|
| `swcn20ww` | BIOS_SWCN1_22000 | SWCN20WW | 639137339907699161 | 2026-05-07T06:59:50.7699161Z | Installed | RebootDelayed | 10 397 320 | `45f03087…` |
| `swcn21ww` | BIOS_SWCN1_22000 | SWCN21WW | 639197187994021876 | 2026-07-15T13:26:39.4021876Z | Installed | RebootDelayed | 10 398 816 | `7526385e…` |
| `udc26605wx64` | UDC_266 | 26.6.0.5 | 639207511543692016 | 2026-07-27T12:12:34.3692016Z | Installed | NoRebootRequired | 24 501 360 | `34d30927…` |
| `swcn22ww` | BIOS_SWCN1_22000 | SWCN22WW | 639237257469503680 | 2026-08-30T22:29:06.9503680Z | Installed | RebootDelayed | 14 075 840 | `020c40c8…` |

`utcinstalldate` is **.NET `DateTime.Ticks`** (100 ns units since 0001-01-01),
not Win32 `FILETIME` (which would place these values in the year 3627). The
decode was verified three ways: the arithmetic matches Python's
`datetime(1,1,1) + ticks`, the resulting dates sit between each package's
release date and today, and the install dates line up with the machine's own
`Uninstall` registry `InstallDate` values (`LenovoMagiCenter 2.0.5.501 →
20260914`, `联想智会 4.2.0.0908 → 20260928`).

`update_history.db` — 30 rows, all `status = AlreadyInstalled`, e.g.

| id | title | version | status | severity | installdate |
|---|---|---|---|---|---|
| `6fge020f6pr889j0_21vg_11_25` | Intel Management Engine Interface (MEI) Driver - 11 | 2552.8.10.0 | AlreadyInstalled | Recommended | *(empty)* |
| `swcn22ww` | BIOS_SWCN1_22000 | SWCN22WW | AlreadyInstalled | Recommended | *(empty)* |
| `qszt0yaf40f0a0_21vg_11_25` | Lenovo Intelligent Sensing (Zero Touch) Driver - 11 | 1.0.6.40 | AlreadyInstalled | Recommended | *(empty)* |

This ledger records catalogue reconciliation, not install events: it has no
install timestamps. `update history` prefers `editable_update_history.db` and
falls back to this one only when the install-event ledger is absent.

### 2.4 What is *not* the install history

* `session\updates.json` has a per-entry `installDate`, but all 24 entries carry
  the **same** value (`639292183707815762` → 2026-11-02T12:12:50.7815762Z), equal to
  the scan time in `UpdateTaskList.xml` (`CreateTime="2026/11/2 12:11:41"`) and
  the file's own write time — it is a scan stamp, not an install date.
* `HKLM\SOFTWARE\Lenovo\DriverUpdate` and its `WOW6432Node` twin hold only
  `InstallPath` / `LenovoInstallPath`; no history values exist on this machine.
* `C:\ProgramData\Lenovo\DriverUpdate` does not exist (the legacy TVSU log root
  named by `tvsutil.dll`: `%s\Lenovo\DriverUpdate\logs\ClearTemp.log`).
  `C:\ProgramData\Lenovo\Udc\Log` exists but is empty.

### 2.5 Vendor package-status ledger (secondary, cited by the report)

`HKLM\SOFTWARE\Lenovo\UDC\PackageStatus\<guid>` holds the UDC service's own
per-package bookkeeping:

| PackageId | PackageVersion | PackageState | InstallationTrigger |
|---|---|---|---|
| `1b4e28ba-2fa1-11d2-883f-0016d3cca427` | 1.3.0.6 | installed | auto |
| `6c28b858-18c6-49f3-9819-b002f41a1492` | 2.0.5.501 | installed | auto |
| `a642ce0d-4c2e-41ba-8a19-de691e5c896a` | 4.2.0.0908 | installed | auto |

It has no timestamps and is written by the service, so it is **not** used as a
post-install readback; it only corroborates what the add-in ledger records.

### 2.6 Schedule / ignore (found, deliberately not implemented here)

* `UpdateSchedule.xml` carries the schedule the add-in last wrote:
  `Frequency=Weekly`, `PackageType=Critical`, `DayOfWeek=Monday`,
  `UTCTimeToCheck=12:11:38`, `SchedulerAbility=ON`,
  `Notify=DownloadAndInstallIfNoReboot`, `BIOSAndFirmwareAutoUpdates=ON`,
  plus the task `\Lenovo\Vantage\Schedule\LenovoSystemUpdateAddin_WeeklyTask`.
* `ProblematicUpdates.xml` is the ignore/exclusion store (`ProblematicPackagesHistory`,
  currently empty).
* `UpdateTaskList.xml` is the session record (`Stage="idle"`,
  `HistoryUpdated="true"`, `LaunchMode="Schedule"`).

Writing these would be a scheduling/authority change (`docs/11-sysupdate-diagnostics.md`
§2.3 describes a `SetSchedule`/`SetIgnoredUpdate` contract), not a read. They
stay `Unsupported`.

---

## 3. Legacy TVSU installers (why no CLI contract was invented)

`Lenovo.DriverUpdate.Client.dll` (376 288 B) and `tvsutil.dll` (105 952 B) under
`…\PCManager\5.1.210.10222\TVSU\` are the legacy TVSU stack:

* installer dispatch: `GenericPackageInstallerFactory`, `PackageInstallerProcess`,
  `CmdInstaller`, `get_InstallCommand`, `get_GetManualInstallCommands`,
  `LDUShim.exe`, `dpinst.exe`, `uts.exe`, `EnumCD.exe`, `tpisysidsu.exe`;
* INF installs go through the Windows service: `"Values to use for the INF
  installation (the windows service is required)-> INF path: "`;
* logging: `Software\Lenovo\DriverUpdate\Preferences\Log`
  (`loggerFileName`, `loggerLevel`), `GetCurrentLogName()`, `\logs\ApplicabilityRulesTrace.txt`,
  `%s\Lenovo\DriverUpdate\logs\ClearTemp.log`;
* `DiDriverInstall64.exe` is only a thin `DiInstallDriverW` /
  `UpdateDriverForPlugAndPlayDevices` wrapper (`.\logs\TVSU_Installer64_error.log`);
  `Installer64.exe` is the GUI host.

Nothing in that stack is a separate, stable command line: `DiDriverInstall64`
is precisely the SetupAPI `DiInstallDriver` path that
`update local-driver apply` already implements in Rust (with WinTrust
verification), so no vendor executable is invoked for INF packages. The numeric
`PackageType.Type` of the legacy package XML is visible in
`available_updates.json` (`Type: 3` for `BIOS_SWCN1_22000`), which is a
different vocabulary from UDC's string `package_type` (`"app"`).

---

## 4. Implemented in this slice

### 4.1 `update history` (read-only)

`UpdateControl::update_history` → `lctrl_hal_win::update_history::read_history`:

* reads `%ProgramData%\Lenovo\Vantage\AddinData\LenovoSystemUpdateAddin\session\editable_update_history.db`
  (falling back to `update_history.db` only when the install-event ledger is
  absent);
* opens it `READONLY` through the OS SQLite engine `winsqlite3.dll`
  (`System32`, 1 181 512 B, exports `sqlite3_open_v2`, `sqlite3_prepare_v2`,
  `sqlite3_step`, `sqlite3_column_count`, `sqlite3_column_text`,
  `sqlite3_column_bytes`, `sqlite3_finalize`, `sqlite3_close`,
  `sqlite3_errmsg`). Loading it by name keeps the crate buildable on images
  without the DLL and on Linux, where the command reports `Unsupported`;
* decodes `utcinstalldate` from .NET ticks to RFC 3339 UTC, keeps absent
  columns absent, bounds rows at 4096, and never writes to the ledger;
* reports which ledger was read (`install_events` / `package_status`), the
  absolute source path, and one record per row.

### 4.2 `update local-install <package-dir>` (contract-only write)

`update local-install …\Udc\Download\<package_id>\<version>\temp` runs the
package's own installer, gated as follows — every gate is checked **before**
anything runs:

| Gate | Behaviour on failure |
|---|---|
| directory is canonical, exists, contains `install.ps1` ≤ 1 MiB | `InvalidArgument` / `Io` |
| directory is exactly `<download root>\<package_id>\<version>\temp` | `InvalidArgument` (path named in the message) |
| package id is in the synced UDC catalogue (`LastUpdateConfig`) | `Unsupported { feature: "update.local-install.package.<id>" }` |
| catalogue entry is not firmware (`package_type` in `bios`/`uefi`/`firmware`, or name contains bios/firmware/uefi) | `Unsupported { feature: "update.local-install.firmware.<id>" }` |
| script Authenticode status is `Valid` **and** the signer subject contains `LENOVO` | `VerifyMismatch` |
| when the script pins `$Global:setup_hash` + `$Global:NewSetupFileName`: payload exists and SHA-256 matches | `VerifyMismatch` |
| process holds an elevated administrator token | `PermissionDenied` |
| installer exits `0` | `VerifyMismatch` carrying the exit code and the log path |

* Runs `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <script> -command install`
  with `CREATE_NO_WINDOW`, no stdin, and a 900 s wait. On expiry the installer is
  **not** killed (a half-applied vendor setup is worse than a reported stall).
* Reports the catalogue name, on-disk version, catalogue type, the verified
  payload hash (or `null` when the script validates itself), the signer
  subject, the installer exit code, and the `%TEMP%\Lenovo*Log-*.txt` the
  script wrote during this run.
* `--dry-run` stops after the gates and reports the plan with `exit_code: null`.
* Never downloads, never schedules, never reboots, and refuses firmware.

Deliberately **not** done: `update download` / `update install <id>` stay
`Unsupported` (mTLS), `update rollback` stays `Unsupported` (the vendor
installer has no CLI rollback and no install ledger binds previous versions),
`update ignore` / `update schedule` stay `Unsupported` (see §2.6).

---

## 5. Limits and inferences

* `[INFERENCE]` The UDC catalogue's `package_type` vocabulary for **firmware**
  packages is unobserved: the synced config only lists `"app"` entries and the
  legacy TVSU schema uses numeric `PackageType.Type`. The firmware refusal
  therefore also checks the package *name*, which does carry `BIOS` on this
  machine (`BIOS_SWCN1_22000`, `BIOS Update Utility - 11`).
* `[UNOBSERVED]` No local package with a present payload exists on the target
  any more (the service deletes payloads after install), so
  `update local-install` **has never been executed end-to-end** against a real
  package. The gates, exit-code handling, and log lookup are contract-derived.
* `[UNOBSERVED]` Whether a package installed outside the UDC service updates
  `HKLM\SOFTWARE\Lenovo\UDC\PackageStatus\<id>` is unknown; the CLI therefore
  does not claim a state readback (`Readback::Unverified` in the capability
  registry) and reports the installer's own exit code and log instead.
* `[INFERENCE]` `winsqlite3.dll` is documented as present on Windows 10 1709+
  and was confirmed on the target; on an image without it `update history`
  reports `ChannelUnavailable` naming the DLL rather than an empty history.

---

## 6. Proposed parent acceptance (and negative controls)

> **Do not run `update local-install` in commit mode on the target as a
> smoke test.** The staged Smart Meeting script begins by running its own
> uninstallers (`unins000.exe` for "Lenovo Smart Meeting Components" and
> "Lenovo Smart Meeting") before reinstalling; a commit test there would
> uninstall/repair a live application. Commit-mode acceptance needs a throwaway
> package, not this machine's installed ones. Everything below is either
> read-only or `--dry-run` (gates only, no process started).

Read-only, safe on the target:

1. `vantage update history` → `ledger: "install_events"`, 4 entries, newest
   first, `swcn22ww` at `2026-08-30T22:29:06.9503680Z`, `udc26605wx64` at
   `2026-07-27T12:12:34.3692016Z`; every entry has an `id`, `status ==
   "Installed"`, no fabricated `severity`.
2. `vantage update history --json` parses; `source` ends with
   `editable_update_history.db`.
3. Negative control — rename the ledger out of the way on a scratch copy or
   point `ProgramData` at a directory without it (do **not** move the real
   file): `update history` must fail with `ChannelUnavailable` naming both
   candidate paths, never an empty success.
4. `vantage update local-install <dir> --dry-run` on
   `…\Download\6c28b858-…\2.0.5.501\temp` → the service deleted that package's
   payload after installing it, so the hash gate must fail with
   `ChannelUnavailable` naming the payload file ("did not produce a SHA-256
   digest"), not silently succeed.
5. Negative control — a directory outside `…\Udc\Download\…` must be refused
   with a message naming the required `<download root>\<id>\<version>\temp`
   layout.
6. Negative control — a scratch copy of a package directory whose
   `install.ps1` is modified (signature no longer valid) must be refused with
   `VerifyMismatch` before any process is started.
7. `vantage capability` on the target must show `update.history` implemented and
   `update.local.package` present; `update download/install/rollback/ignore/schedule`
   must stay unsupported.

---

## 7. Artifacts

Target-side scratch (not committed): `upd_recon1.txt` … `upd_recon6.txt`,
`recon_update_hist*.ps1` under `C:\Users\ezra\`.

Copied for inspection (host scratch):

| Artifact | SHA-256 |
|---|---|
| `editable_update_history.db` | `fac1e0d8bf960b896ad74a9e3eb3d7e465b75141809008645904dac73198d490` |
| `update_history.db` | `cc55911926acccd166f5949d2aee502ae2e6e2a3fe64ec7c7f6f97998041777f` |
| `6c28b858…/install.ps1` (MagiCenter) | `f27a6e69f267eff22580afa1494795de8894b1d8e9c03d111e2b5f364ff65fa5` |
| `a642ce0d…/install.ps1` (Smart Meeting) | `caf169fac8aa07f5eee79c41cd3de9b63a50bd97c904140da2e988a06b9408d1` |
| `System32\winsqlite3.dll` | `a9148fbbbb59e40e0be60b6f0f85d286ac55a4558a8a3b1e4ccdcac41bfacfab` |

# Privacy camera write + status — implementation record — 2026-11-05

Channel recon: [`re-privacy-osd-20261105.md`](re-privacy-osd-20261105.md).
Scope of this record: the **runtime PnP camera channel** (`privacy cam …`) only.
Persistent BIOS `IntegratedCamera` staging/save is unchanged and documented in
`04-peripherals.md` §4.1.

## 1. Contract (what the vendor actually does)

21VG camera privacy is **devnode enable/disable**, not an EC/GPIO shutter and not
a WMI class. Owner binary
`C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\Addins\LenovoMachineFixSystemAddin\…\CameraModule.dll`
carries the symbols `DriverHelper::EnableByPropertyChange` and
`StartDevice: CM_Enable_DevNode Success for device '%s'`, i.e. the vendor path is
`CM_Enable_DevNode`/`CM_Disable_DevNode` on the camera devnode. The recon found
no `Lenovo_SetCameraStatus`/`Lenovo_CameraStatus` WMI class under `root\wmi`,
and no shutter GPIO.

Reference-machine devnodes (§recon): `Integrated Camera`
(`USB\VID_174F&PID_182B&MI_00`) and `Integrated IR Camera` (`…&MI_02`) under one
USB parent; `Lenovo Camera Group`, `Windows Studio Effects Camera`,
`Camera DFU Device` (`…&MI_04`) are kept out of the admitted group by the
selector's internal-identity rules.

## 2. Write path — already implemented, target-verified 2026-10-30

Source (`crates/lctrl-hal-win/src/runtime_devices.rs`):

| Requirement | Where |
|---|---|
| Admit exactly one physical camera (RGB+IR together, ambiguity refused) | `select_runtime_devices` / `camera_group` |
| Nothing written on `--dry-run` | `set_group` (`ApplyMode::DryRun` returns before any `set_state`) |
| Write through the vendor primitive | native `set_state`: `CM_Disable_DevNode(devinst, CM_DISABLE_UI_NOT_OK)` / `CM_Enable_DevNode(devinst, 0)` (`:1025`, `:1030`) |
| Confirm: readback must reach the requested state | `poll_readback` (`crates/lctrl-hal/src/verify.rs`) — 10 × 50 ms, returns `VerifyMismatch { requested, actual }` on exhaustion |
| Rollback on partial failure or mismatch | `rollback_group` restores every already-attempted devnode and re-reads each; a rollback failure becomes `FirmwareRejected` |
| Pre-write refusals never trigger a bogus rollback | `preserve_prewrite_error` (`PermissionDenied`/`Unsupported`/`InvalidArgument`) |

CLI: `vantage privacy cam on|off --runtime` → `execute_runtime_camera`
(`crates/vantage-cli/src/lib.rs:6474`) prints `requested` / `actual` /
`previous`; `--auto-restore` re-runs the same guarded transaction for the
captured previous state.

Live acceptance already recorded on the reference machine (2026-10-30,
`04-peripherals.md` §4.2): dry-run had no side effect; the committed disable
moved both USB camera interfaces from OK to Error as seen by an independent
`Get-PnpDevice -Class Camera`, and enable returned them to OK. That record is
**not** re-claimed here.

## 3. Added by this change

### 3.1 Per-devnode camera read (`privacy cam status`)

`camera_state()` deliberately refuses to answer for a group whose devnodes
disagree (`aggregate_group_state` → `ChannelUnavailable`). That is right for a
write, but it hides *which* collection is off — the exact situation an operator
needs to see. New read, no writes:

- `lctrl_core::PrivacyDeviceState { instance_id, description, state, error }`
  (`crates/lctrl-core/src/privacy.rs:46`).
  `state` is `None` when the platform reports a PnP status that is neither
  started nor `CM_PROB_DISABLED` (`PnpDeviceStatus::state` refuses those);
  `error` carries that observation, so a broken devnode is attributed instead
  of being collapsed into a fake `enabled`/`disabled`.
- `PrivacyControl::camera_devices` (`crates/lctrl-hal/src/peripherals.rs:49`)
  with a defaulted `Unsupported { feature: "privacy.cam.devices" }` body —
  same shape as `TouchpadControl::recover_touchpad`, so the Linux backend and
  existing fakes stay honest without edits.
- `WindowsRuntimeDevices::camera_devices`
  (`crates/lctrl-hal-win/src/runtime_devices.rs:459`): enumerate the admitted
  group, read `CM_Get_DevNode_Status` per instance (`:1006`), attribute each.
  No `set_state` call exists on this path.
- CLI `privacy cam status` (`execute_camera_status`,
  `crates/vantage-cli/src/lib.rs:6432`) reports, read-only:
  `runtime` (aggregate `DeviceState`) + `runtime_error` (split/inconclusive
  group), `devices` (per-devnode) + `devices_error`, `persistent`
  (BIOS `IntegratedCamera` item), `occlusion` (see §4). `--runtime`/
  `--persistent` are rejected: the read always covers both layers.

Existing spellings are unchanged (`privacy cam on|off --runtime|--persistent`,
`privacy status`, `privacy mic …`). Added spellings for the item's wording:
subcommand alias `camera` and value aliases `enable`/`disable`, following the
`touchpad {status|on|off|recover}` precedent
(`TouchpadAction` uses the same `#[value(alias = "enable")]` form).

`command_is_mutating` now excludes `PrivacyCamAction::Status`, so the status
read does not run the mutating-command conflict preflight.

### 3.2 Capability registry

No new row. `privacy cam status` carries no layer flag and therefore maps to no
`CommandSelector`, exactly like `privacy status` (`capability::privacy_layer`
returns `None` for both). `PRIVACY_CAM` keeps its two existing rows (runtime
mutation, BIOS-persistent mutation).

## 4. Occlusion — deliberately reported as unsupported, not faked

`Windows.Media.Devices.CameraOcclusionInfo` (`GetState()`,
`IsOcclusionKindSupported`, `StateChanged`; UniversalApiContract v14, Windows 11)
is obtainable **only** from `VideoDeviceController.CameraOcclusionInfo` of an
initialized WinRT `MediaCapture` session (learn.microsoft.com,
`windows.media.devices.cameraocclusioninfo`). Consequences on this channel:

1. Reading it opens the camera, i.e. a status command would contend with the
   user's capture apps and would fail whenever the devnode is disabled — one of
   the two states this tool controls.
2. Microsoft's own remarks: the shutter state is to be used only while the
   camera is actively streaming and **must not** be treated as an absolute
   privacy indicator.
3. The 21VG recon found no shutter hardware, so the value would be absent or
   constant even with a session.

Therefore `privacy cam status` reports
`occlusion: { "supported": false, "reason": "CameraOcclusionInfo requires an initialized WinRT MediaCapture session; no shutter hardware on 21VG" }`
(`CAMERA_OCCLUSION_REASON`, `crates/vantage-cli/src/lib.rs:1325`). No WinRT/
`windows`-crate dependency is added; nothing here fabricates a shutter state.
Implementing the session is a separate, parent-owned decision (new dependency,
camera activation per call, live verification on the target).

## 5. Authority and lifecycle

- Owner binary/registry authority: `CameraModule.dll` symbols (recon §Camera
  privacy) + the devnodes enumerated by SetupAPI class
  `{CA3E7AB9-B4C3-4AE6-8251-579EF933890F}` on the reference machine.
- Lifecycle: runtime PnP state — survives until the device is re-enabled, the
  devnode is re-enumerated, or the OS restarts; independent of the BIOS
  `IntegratedCamera` switch, which needs a reboot and is reported separately as
  `persistent`. The two layers can disagree, which is why `status` reports both
  instead of one boolean.
- Nothing in this change writes: `camera_devices` and `execute_camera_status`
  contain no `set_state`/`stage`/`save` call.

## 6. Acceptance — designed, NOT run by this worker

Source-bound checks for the parent (this worker ran no build, test, formatter,
or target program):

1. `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc`
   (new inherent + trait method, default body on the Linux side).
2. `cargo test -p lctrl-hal-win --test windows_runtime_devices` — new
   `camera_devices_attributes_a_split_group_per_devnode` (RGB enabled + IR
   disabled → two entries, per-instance states, **no** writes), plus the
   existing `camera_group_split_state_is_not_reported_as_one_answer` must still
   pass (aggregate unchanged).
3. `cargo test -p vantage-cli --test cli_contract` — new
   `camera_status_reports_both_layers_without_mutating` (runtime `enabled`,
   `persistent.value` `Enable`, `devices_error.kind` `unsupported` for a fake
   without the channel, `occlusion.supported` false, no BIOS staging/save) and
   `camera_alias_accepts_enable_and_disable_spellings`
   (`privacy camera disable --runtime` parses to `Cam { action: Off, runtime: true }`).
4. `cargo test -p vantage-cli --test cli_capability` — `privacy cam off
   --runtime`/`--persistent` selectors unchanged; `privacy cam off` still maps
   to no selector.
5. Target (unrun, needs an elevated console session):
   - `Get-PnpDevice -Class Camera | Select InstanceId,Status` before/after
     `vantage privacy cam status --json` — must be identical (no mutation), and
     the JSON `devices[]` instance IDs must equal that list (expected
     `USB\VID_174F&PID_182B&MI_00`, `…&MI_02`; MI_02 identity is `[INFERENCE]`
     from the recon's "Integrated IR Camera" entry).
   - `vantage privacy cam off --runtime --json` → `actual: disabled`; status
     then shows `runtime: disabled` and both `devices[].state: disabled`.
   - Recovery: `vantage privacy cam on --runtime --json` → `actual: enabled`.
   - Negative control: `vantage privacy cam status --runtime` must fail with
     exit code 2 (`invalid_argument`) and touch nothing.
   - Negative control: `vantage privacy cam status --json` with the service
     absent must fail `Unsupported` (`privacy.cam.status`), not report empty
     success.

## 7. Open items

- Occlusion session (WinRT `MediaCapture`) — parent decision, see §4.
- IR/RGB instance-ID spelling in §6.5 is inferred from the recon; confirm from
  the live `devices[]` output before pinning it in any doc table.

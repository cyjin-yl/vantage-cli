# Touchpad channel — contract recon and `recover` semantics — 2026-11-05

Target: `TB14P-EZRA` / LENOVO 21VG (ThinkBook 14 G8+ IPH), firmware `SWCN22WW`.
Scope: the touchpad switch (`touchpad`), its read-only aggregate
(`touchpad.status`) and the repair verb (`touchpad.recover`). Read-only recon;
the only write this document authors is the repair contract below.

## 1. Vendor plane: the documented contract, and why it is dead on this SKU

Specified contract (`docs/01-hal-interfaces.md` §2.2, `docs/04-peripherals.md` §1.4):

```text
IsSupportDisableTP(out Data)            capability probe
GetTPStatus(out Data: UInt32)           0=enabled 1=disabled
SetTPStatus(Data: UInt32) -> Boolean    immediate, no reboot
```

Proving countersignature on the target (both receipts are already in the repo —
no new probe was run for this document):

- `docs/evidence/wmi-capability-exhaustion-20261104.md` §4 — a per-call instance
  binding dispatches, and `IsSupportGpuOC` answers, while
  `IsSupportDisableTP`, `GetTPStatus` and the rest of the class return
  `无效的对象` (`WBEM_E_INVALID_OBJECT`). The discriminator is that one method
  answers and its siblings reject, so the rejection is per-method and not a
  provider/binding fault.
- `docs/evidence/capability-terminal-matrix-20261104.md` §4 — `SetTPStatus` was
  dispatched and the touchpad stayed `OK`; the write plane is an empty
  implementation because the firmware never loaded the Legion `GMZN` ACPI
  objects.

Consequence: there is **no Lenovo user-mode binary to reverse for the touchpad
write** on 21VG; the door is closed at the ACPI/WMI plane, below any DLL. A
DLL/offset-based implementation would be a fiction here.

## 2. Implemented plane: public Win32 PnP configuration manager

The replacement drives the input leaves through system components, not Lenovo
code:

| Binary | Imports used (`crates/lctrl-hal-win/src/runtime_devices.rs`, `mod native`) |
|---|---|
| `cfgmgr32.dll` | `CM_Locate_DevNodeW`, `CM_Get_DevNode_Status`, `CM_Disable_DevNode`, `CM_Enable_DevNode`, `CM_MapCrToWin32Err` |
| `setupapi.dll` | `SetupDiGetClassDevsW`, `SetupDiEnumDeviceInfo`, `SetupDiGetDevicePropertyW` (identity: `HardwareIds`, `CompatibleIds`, `Parent`, descriptions) |

Contract details that the code depends on:

- `CM_Disable_DevNode(devinst, CM_DISABLE_UI_NOT_OK)` — the flags deliberately
  exclude `CM_DISABLE_PERSIST`, so a disable is **runtime-only** and cannot
  survive a reboot. `CM_Enable_DevNode(devinst, 0)` re-enumerates the node.
- Errors are mapped from `CONFIGRET`: `CR_ACCESS_DENIED`→`PermissionDenied`,
  `CR_NOT_DISABLEABLE`/`CR_CALL_NOT_IMPLEMENTED`/`CR_INVALID_API`→`Unsupported`,
  `CR_NO_SUCH_DEVNODE`/`CR_INVALID_DEVNODE`/`CR_DEVICE_NOT_THERE`/`CR_NO_CM_SERVICES`→
  `ChannelUnavailable`, `CR_NEED_RESTART`→`FirmwareRejected`.

Admission (unchanged by this work, and deliberately narrow):

- exactly one `HIDClass` devnode carrying `HID_DEVICE_UP:000D_U:0005`
  (digitizer/touchpad usage); a name, `PNP0C50` or an ACPI parent alone is never
  enough;
- plus `Mouse`-class leaves carrying `HID_DEVICE_UP:0001_U:0002` whose
  `DEVPKEY_Device_Parent` equals the digitizer's parent, capped at 16 leaves;
- the ACPI controller, the configuration collection and the vendor-defined
  collection are **never** targeted (`docs/04-peripherals.md` §1.4); disabling
  the controller removes the children needed to rediscover the device, which is
  exactly the incident recorded in `docs/C-acceptance.md` C7.

Live topology observed on 2026-10-27 (`docs/evidence/touchpad-recovery-20261027.json`):

```text
ACPI\GXTP5100\4&30ed0a95&0    I2C HID Device  hidi2c.inf  (controller, parent
                                                            PCI\VEN_8086&DEV_E478)
HID\GXTP5100&Col01\...        HID-compliant mouse        usage 0001_U:0002
HID\GXTP5100&Col02\...        HID-compliant touch pad    usage 000D_U:0005
HID\GXTP5100&Col03\...        configuration collection
HID\GXTP5100&Col04\...        vendor-defined collection
```

The admitted group is `Col02` (digitizer, group[0]) + `Col01` (mouse, group[1]).

## 3. Field failure mode the repair verb has to address

Recorded in prior sessions on this machine (2026-11-04/05): the pad keeps
serving pointer motion and multitouch, while **physical press and haptic
feedback die together**; the vendor-defined collection (`Col04`, Goodix GTX8
firmware tunnel: report `0x0E`, `I2C_DIRECT_RW 0x20`, firmware read at
`0x452C`) stops answering feature reads. `pnputil /restart-device` on the
`GXTP5100` node and on the `E478` I²C parent both returned success with
`ProblemCode=0` and did not restore press/haptic.

`[INFERENCE]` That signature is an MCU/firmware hang, not a PnP state change:
every admitted leaf stays `Started`, so the previous implementation of
`recover` ("enable the leaves that report Disabled") was a no-op in precisely
the state the verb exists for. `[INFERENCE]` because no HID-level receipt was
retained in this worktree; the PnP-state half is observed.

## 4. Repair contract as implemented (this change)

Strategy is derived from the observed leaves, never from a caller flag, and the
two paths are never mixed:

| Observed leaves | `strategy` | Writes |
|---|---|---|
| at least one `Disabled` (interrupted transition / split state) | `enable_disabled_leaves` | enable only the disabled leaves; independent readback per leaf |
| every leaf `Enabled` (enumerated but not serving input) | `cycle_input_leaves` | per admitted leaf, `CM_Disable_DevNode` → verify the restart was observed (bounded tolerant poll) → `CM_Enable_DevNode` |

Both paths end with an independent readback of **every** leaf to `Enabled`,
using a longer bounded window than an ordinary transition (a HID collection is
re-enumerating). A leaf that fails the readback gets one explicit re-enable and
one re-read, so recovery never leaves an input leaf disabled; a remaining
failure is reported as `FirmwareRejected` naming the leaf, and the verb can be
re-run.

Dry-run performs no PnP write and reports the strategy plus each leaf's
`previous`/`requested`.

CLI surface: `vantage touchpad {status|on|off|recover}`; `enable` and
`disable` are accepted as aliases of `on` and `off`, so `touchpad enable`,
`touchpad disable` and `touchpad recover` all parse. The human output names the
strategy that ran and states that the readback proves enumeration, not
press/haptic function.

Safety boundaries preserved: runtime-only disables (no `CM_DISABLE_PERSIST`), no
controller write, no `Col03`/`Col04` write, no driver install/uninstall, no
reboot, and no other mouse device touched.

Invoking `recover` on a healthy pad therefore restarts its two input
collections. That is intentional: the verb is the "this pad is sick, restart
it" operation, and the restart is the only repair available on the public
plane.

## 5. Explicit limits

- PnP readback proves the devnode is enumerated and reporting `Enabled`. It does
  **not** prove press, haptic or pointer function; only the operator can.
- Because the vendor firmware-tunnel collection is excluded by the documented
  boundary, a leaf cycle cannot reset the Goodix MCU. `[INFERENCE]` An MCU-level
  hang may still require a full shutdown/power-on.
- Split-state discovery still fails closed: if the digitizer or the mouse leaf
  cannot be uniquely identified, or the leaf count exceeds 16, recovery refuses
  before any write.

## 6. Acceptance that was NOT run (parent owns it)

Source-only change in this worktree; no build, test or target execution was
performed here.

Positive checks on the target (admin shell, batching into one `.ps1`):

1. `vantage touchpad status` → `enabled` (read-only; no `writes`).
2. `vantage --dry-run touchpad recover --json` → `strategy:"cycle_input_leaves"`,
   both leaves `previous:"enabled"`, `actual` absent, and independent
   `pnputil /enum-devices` shows the controller and all four collections
   unchanged.
3. `vantage --yes touchpad recover --json` → `strategy:"cycle_input_leaves"`,
   both leaves `actual:"enabled"`; independent enumeration right after shows
   `ACPI\GXTP5100\4&30ed0a95&0`, `Col01`, `Col02` still `Started/Problem 0`, and
   `Col03`/`Col04` byte-identical to the pre-run snapshot.
4. Operator reports whether press/haptic came back (the only functional proof;
   record it either way — a negative result is a contract limit, not a bug).
5. Split-state path: with one leaf disabled out of band
   (`pnputil /disable-device`), `touchpad recover` → `strategy:"enable_disabled_leaves"`
   and exactly one enable write per disabled leaf.

Negative controls:

- `touchpad status` with a vendor controller running must still be read-only
  (existing CLI test `touchpad_status_is_read_only_even_with_a_vendor_controller_running`).
- `touchpad recover` without `--yes` while `powershell.exe`/Vantage control is
  detected must still be refused by the conflict preflight.
- A pad whose mouse leaf is missing must fail with `ChannelUnavailable` before
  any write (existing test `touchpad_recovery_rejects_an_unverified_missing_mouse_collection`).
- Injected enable failure must end with the leaf `Enabled` and the verb
  non-zero (new test `touchpad_recovery_cycle_failure_restores_the_leaf_and_reports`).

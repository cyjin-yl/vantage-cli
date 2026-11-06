# Fn hotkeys — owner components and the state that is actually readable — 2026-11-05

Target: `tb14p-ezra` = ThinkBook 14 G8+ IPH (21VG), Windows 11 Pro for Workstations
(26100), Lenovo Fn package 2.1.2602.8
(`lenovofnandfunctionkeys.inf_amd64_5e21bf389d23855a`).
Work item: HOTKEYS — `vantage hotkey status` (read-only).
Scope: recover the Fn hotkey state the machine can answer. **No remap write is
implemented**, no service was started/stopped, no registry value was written, and no
vendor program was launched. Every value below came from a read-only registry / SCM /
process / `root\wmi` query.

`vantage hotkey list` reports only this process's own `RegisterHotKey` set, so on a
machine where nothing is registered it prints `0 live global hotkey(s)` while the Lenovo
Fn stack is fully running. This note pins what the machine can actually answer for the
*Lenovo* side of the same question.

## 1. Owner components (live inventory)

| component | path / identity | version | SHA-256 |
| --- | --- | --- | --- |
| `LenovoFnAndFunctionKeys` service | `ImagePath = ...\lenovofnandfunctionkeys.inf_amd64_5e21bf389d23855a\LenovoUtilityService.exe`; `Start=2`, `Type=16`, `ObjectName=LocalSystem`, state `Running` | `UtilityVersion = 2.1.2602.8` (service key) | `FE4590DC6402B85442C7899B37046FA752A1C12DF80A48D90758F26C3D001D99` |
| `LenovoUtilityService.exe` | same package, PID 6304, session 0 | 2.1.2602.8 | `FE4590DC6402B85442C7899B37046FA752A1C12DF80A48D90758F26C3D001D99` |
| `FnHotkeyUtility.exe` | same package, PID 15896, **session 1** | 2.1.2602.8 | `FB326C9CDEA357BB174D6A9C3FECD60F089C036176803164AAFF78A59AD22A32` |
| `FnHotkeyCapsLKNumLK.exe` | same package, PID 2748, **session 1** | 2.1.2602.8 | `7934CB3EFFFD6AB002404DCB04BA3BA6793D379C2491BD64EEEC2126CA322492` |
| `ludp.dll` (HID/keyboard helper imported by the renderer) | same package | — | `D7F0979294D5A2A3C7197D4CA929E260BAC92B32CC4477828D4AE7893ECD6AD2` |
| `ControlLib.dll` | same package | — | `B4B92AEE9A1EFE675E264DEB688B04D8EE19F61D836E12636335C67600376408` |

The service key carries no `LenovoUtilityService` sibling — the package registers exactly
one service; the interactive renderer is launched by it
(`CUtilityService::LaunchFnHotkeyUtilityProcess` strings in `LenovoUtilityService.exe`).

## 2. Readable state

### 2.1 Fn-lock (function-row mode) — BIOS settings, read live

`root\wmi:Lenovo_BiosSetting` (read-only enumeration), live values on this machine:

| setting | live value |
| --- | --- |
| `HotkeyMode` | `Disable` |
| `F1-F12AsPrimaryFunction` | `Enable` |
| `FoolProofFnCtrl` | `Enable` |
| `FnAndCtrlKeySwap` | `Disable` |

The vendor mapping (`docs/04-peripherals.md` §1.2, `docs/05-bios-settings.md` §4.2) is:

| HotkeyMode | F1-F12AsPrimaryFunction | behaviour | FnLock |
| --- | --- | --- | --- |
| `Enable` | `Disable` | press = media; Fn+Fx = F-keys | off |
| `Disable` | `Enable` | press = standard F1-F12; Fn+Fx = media | **on** |

So the live Fn-lock state is **on** (`HotkeyMode=Disable` + `F1-F12AsPrimaryFunction=Enable`).
Any other pair is reported as `unknown` rather than guessed.

The utility itself does not own this state: `FnHotkeyUtility.exe` only paints the OSD for
the firmware-generated event (`e_Event_FNLK_ON` / `e_Event_FNLK_OFF`, plus the localized
`"FnLock function is enabled/disabled"` strings). The pull-based, persisted expression is
the BIOS pair above; the live EC/LED bit is firmware-owned and is not exposed to user mode
(the ACPI handler described in `docs/09-linux-backend.md` §4.5 touches EC `0x4A` bits
5/6, and there is no read path to that register without a driver) [INFERENCE for the EC
ownership; the BIOS pair is measured].

### 2.2 Fn/Ctrl swap — BIOS setting, read live

- `FoolProofFnCtrl = Enable` on this machine ⇒ **swapped**. This is the item
  `vantage kbd fn-ctrl-swap` writes (`docs/04-peripherals.md` §1.1).
- `FnAndCtrlKeySwap = Disable` is a sibling item present on the same SKU; it is reported
  as an observed sibling and is never written by our commands.
- The swap is **not** a registry value: `reg query HKLM\SOFTWARE\Lenovo /s /f FnLock`
  and the same search over `HKCU\SOFTWARE\Lenovo` return 0 matches. It exists only as
  the firmware setting (BIOS WMI text), so the status reads it there. When
  `FoolProofFnCtrl` is absent the status falls back to `FnAndCtrlKeySwap` and says so.

### 2.3 Capability flags the hotkey package keys its behaviour on

`HKLM\SYSTEM\CurrentControlSet\Services\LenovoFnAndFunctionKeys\Capbility` (live):

| value | live | meaning |
| --- | --- | --- |
| `SupportFnLockVer` | `1` | the package advertises the FnLock feature |
| `SupportKeyboardLocationLayout` | `1` | keyboard layout service (`SVC_CTL_SET_KEYBOARD_LAYOUT`) supported |
| `SupportNumlkKey` | `0` | no NumLk key on this SKU (also gates the NumLk OSD) |
| `IsUserChangedCapsNumLKStatus` | `1` | user has taken the Caps/Num OSD decision over |
| `ITSMajorVersion` | `5` | ITS dispatcher interface version |
| `IsSMBMachine` | `1` | SMB (commercial) machine flag |

`hotkey status` reports the four flags a Fn-lock decision depends on and leaves the rest
in the registry where the utility reads them.

### 2.4 Hotkey tables — persisted per-model key maps (not the live keyboard)

| store | contents on this machine |
| --- | --- |
| `HKCU\SOFTWARE\Lenovo\Hotkey` | **37** device subkeys (`VID_0461&PID_4F2B`, 35× `VID_17EF&PID_xxxx`, `VID_1915&PID_EEE0`), each with exactly **21** `REG_BINARY` values: `05`, `Ex_1A`, `Ex_1B`, `Ex_20`, `Ex_25`, `Ex_27`, `Ex_31`, `Ex_40`…`Ex_45`, `Ex_91`, `Ex_99`…`Ex_9F` |
| `HKLM\SOFTWARE\Lenovo\LenovoFnHotkey\HIDKBMicMuteLED` | **3** device subkeys (`VID_0461&PID_4F2B`, `VID_17EF&PID_616A`, `VID_17EF&PID_616B`), each with **11** values (`05`, `Ex_1A`, `Ex_1B`, `Ex_20`, `Ex_25`, `Ex_27`, `Ex_31`, `Ex_91`, `Ex_99`…`Ex_9B`) |

Sample payload (`HKCU\...\VID_0461&PID_4F2B`):

```text
Ex_31 0C0103000100   Ex_1A 0C0103000200   Ex_91 0C0103080000
05    0C0103004000   Ex_20 0C0103100001   Ex_25 0C0103100010
Ex_27 0C0103100012   Ex_1B 0C0103100018   Ex_99 0C010310002D
```

Provenance: `FnHotkeyUtility.exe` embeds the whole path list — 37
`Software\Lenovo\Hotkey\VID_…&PID_…` strings (1× `VID_0461`, 35× `VID_17EF`, 1×
`VID_1915`), exactly the 37 tables the registry holds — and reads per-code `Ex_*` values;
`LenovoUtilityService.exe`/`ludp.dll` carry the mic-mute LED subset.

**The tables do not describe the connected keyboard.** The internal keyboard enumerates
as `HID\INTC816&COL01` (I²C HID) and `HID\GVINPUT&COL03` (translated device); neither
instance id is in the table, and no `VID_0461`/`VID_17EF` device is present in
`HKLM\SYSTEM\CurrentControlSet\Enum\HID`. The table is the utility's compatibility list of
Lenovo keyboard models, written for whichever model it last matched. Payload semantics
beyond "per-`Ex_`-code report template" are **not** proven [INFERENCE], so the status
reports device, scope and mapping count, never a decoded meaning.

`HKCU` is the *invoking* user's hive: the SSH test account is the same profile as
session 1, so it sees the 37 tables the renderer wrote; a session-0 daemon running as
another account sees its own (usually empty) copy and reports that honestly.

### 2.5 Negative results (probed, not assumed)

- **No `FnLock` registry value exists** anywhere under `HKLM\SOFTWARE\Lenovo` or
  `HKCU\SOFTWARE\Lenovo` (`reg query … /s /f FnLock`: `0 match(es) found`).
- `root\wmi:LENOVO_FEATURE_STATUS_DATA` exists as a class (`Active`, `IDs`, `Status`) but
  has **no instance** on this machine — no feature-status vector to read.
- Every enumerated `Lenovo_FunctionRequest` instance (`ACPI\PNP0C14\ISET_0` …) reports an
  empty `CurrentSetting`; the class is the write target for function requests, not a
  state source.
- `LENOVO_BIOS_ASSISTANT.GetCapabilityValue`, `…GetValue` and
  `Lenovo_GetBiosSelections.GetBiosSelections` reject PowerShell's untyped out-parameters
  (`无效的方法参数`, `无效参数"Data"`). They are reachable through the project's typed
  Rust WMI client (`bios assistant`), but nothing in this slice depends on them.
- `FnHotkeyUtility.exe` imports **no** `hid.dll`/`hidsdi.dll` (only `setupapi`,
  `ludp.dll`, `spkvol.dll`, …). Its FnLk/sticky observation is raw-input driven
  (`HidKeyboardMsgCenter HID_RawInputDataProcess HidGetFnLkOrStickyState` log strings):
  the vendor learns the state from key events. No pull-based HID feature report was found,
  so no HID read is implemented.

### 2.6 Static action vocabulary (recovered, not machine state)

The renderer dispatches keys to a fixed event enum in the binary. Recovered names that
describe the Fn side of this machine (`strings -el` on `FnHotkeyUtility.exe`):

```text
e_Event_FNLK_ON / e_Event_FNLK_OFF                    Fn-lock OSD
e_Event_KBDBRIGHT_OFF / _L1 / _L2 / _L3               keyboard backlight levels
e_Event_KBD_LED_OFF / _LOW / _HIGH / _AUTO            backlight presets
e_Event_PLUGIN_LED_FNU_ON / _OFF                      Fn-U plugin LED
e_Event_Backlight_HID / e_Event_Backlight_new         HID backlight path
e_Event_CAMERA_SWITCH_ON / _OFF, e_Event_Mic          camera/mic keys
e_Event_PANEL_RFESHRATE, e_Event_AC_DC_HIGHRATE/LOWRATE   refresh-rate key
e_Event_PTP_TOUCHPAD, e_Event_NumberLock, e_Event_CapsLock
e_Event_HID_SUPERKEY, e_Event_LSK / _LSK_2_WMI, e_Event_Mouse_ITS
e_Event_OnekeyStudio_ITS, e_Event_OCR_LAUNCH, e_Event_ALEXA_LAUNCH_KEY
e_Event_SMARTCONNECT_KEY, e_Event_MAGICMOVER_KEY, e_Event_CUSTOMIZE_KEY
```

These are compile-time action identifiers, not a stored, user-remappable table: no registry
value or WMI object on this machine carries a key→action mapping (the `Ex_*` tables in §2.4
are report payloads). `hotkey status` therefore reports them as *evidence of the action
set*, not as state, and no remap verb is implemented.

## 3. What `hotkey status` reports (and what it refuses to)

Read-only, total (a machine without the Fn package answers with `service_running=false`
instead of an error):

| field | source |
| --- | --- |
| `service`, `service_running` | SCM (`LenovoFnAndFunctionKeys`) |
| `utility_version` | service key `UtilityVersion` |
| `utility_running`, `caps_led_running` | Toolhelp process snapshot for `FnHotkeyUtility.exe`, `FnHotkeyCapsLKNumLK.exe` |
| `capabilities.*` | `Capbility` DWORDs §2.3 |
| `fn_lock` | `Lenovo_BiosSetting` pair §2.1 |
| `fn_ctrl_swap` | `Lenovo_BiosSetting` pair §2.2 |
| `tables` | registry enumeration §2.4 |
| `notes` | every source that could not be read, verbatim |

Nothing is written: no service control code, no `SetBiosSetting`, no registry write, no
keyboard remap. `hotkey register/trigger/unregister` keep their existing meaning (the
process's own `RegisterHotKey` set), and no Fn/Ctrl or FnLock write is added here — those
remain `kbd fn-ctrl-swap` / `kbd fnlock` (BIOS, confirmed + reboot).

The capability probe behind `vantage capability` uses the cheaper variant of the same
read (service, `Capbility`, renderers, tables) and skips the firmware query, because that
probe already enumerates `Lenovo_BiosSetting` once for `channel.bios`.

## 4. Acceptance still to be run by the parent

- `vantage hotkey status --json` on `tb14p-ezra` in an interactive session: expect
  `service_running=true`, `utility_running=true`, `fn_lock.state="on"`,
  `fn_ctrl_swap.state="on"`, `tables` = 37 current-user + 3 machine tables.
- Same command over SSH (session 0): still must report the registry/BIOS state; the
  renderer check is machine-wide, so it stays `true` as long as session 1 runs.
- Negative control: on a machine without the Fn package the command must answer
  `service_running=false`, `utility_running=false`, `fn_lock.state="unknown"` and notes
  instead of an error.
- No physical key was pressed; Fn+Esc / Fn+Ctrl behaviour is not re-verified here.

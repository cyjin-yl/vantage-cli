# Channel misread sweep, 21VG ThinkBook 14 G8+ IPH — 2026-11-04

Read-only reconnaissance for channels reported unsupported/unavailable because
the CLI reads a name, class, or key that this machine does not publish. The
archetype is `performance.state`, which read `VERSION`/`CURRENT_SETTING`/
`CURRENT_STATE`/`AUTOMATIC_MODE_SETTING`/`POWER_SLIDER` while the installed
dispatcher publishes `ITS_CurrentSetting` and friends. That one is fixed
(commit `4d439a5`). This file records the remaining candidates.

Build under test: `C:\Users\ezra\vantage-target\release\vantage.exe`.

## Verdict table

| Channel | CLI reads (file:line) | Machine publishes | Verdict |
| --- | --- | --- | --- |
| `performance.state` | PowerSlider `VERSION`/`CURRENT_SETTING`/`CURRENT_STATE`/`AUTOMATIC_MODE_SETTING`/`POWER_SLIDER` | `ITS_CurrentSetting`, `ITS_CurrentSettingV`, `ITS_AutomaticModeSetting`, `ITS_HW_Capability`, `ITS_FN_Capability`, `ITS_FanMode` | **MISREAD — fixed** |
| `keyboard.backlight` | `LENOVO_LIGHTING_DATA WHERE Lighting_Id = 0` then `LENOVO_LIGHTING_METHOD.Get_Lighting_Current_Status` (`crates/lctrl-hal-win/src/peripherals.rs:9-11,36,58-64`) | Nothing: the class answers `不支持` (WBEM_E_NOT_SUPPORTED) for the query with **and** without a `WHERE` clause | **FIRMWARE ABSENT** |
| `fan.mode` | `LENOVO_GAMEZONE_DATA.GetSmartFanMode` (`crates/lctrl-hal-win/src/gamezone.rs`, `thermal.rs`) | `GetSmartFanMode` → `无效的对象`; `LENOVO_FAN_METHOD` → `不支持`; registry carries `ITS_FanMode=4` | **FIRMWARE ABSENT** (WMI); registry value is a dispatcher-internal projection, see below |
| `battery.thresholds` | no vendor write contract | No charge-threshold name anywhere: 53 `Lenovo_BiosSetting` entries contain only `AlwaysOnUSB`/`ChargeInBatteryMode`; a recursive scan of `HKLM\SYSTEM\CurrentControlSet\Services\*\*` and `HKLM\SOFTWARE\Lenovo` for `Charge|Threshold|Conserv|Battery` found only `Services\ACPI\Parameters\BatteryFeaturesGranted = 01 00 00 00` and unrelated OS counters | **FIRMWARE ABSENT** |
| `bios.save` | returns `Unsupported("bios.save.global-buffer")` (`crates/vantage-cli/src/lib.rs:6738-6740`) | `Lenovo_SaveBiosSettings.SaveBiosSettings(parameter:String, return:String)` exists on `ACPI\PNP0C14\ISET_0`, and the HAL already implements it (`crates/lctrl-hal-win/src/bios.rs:100-111`) | **DELIBERATE BOUNDARY** — see below |
| `update.*` | no catalog contract attached | Lenovo updater stack **is** installed: registry keys `DriverUpdate`, `SystemUpdateAddin`, `UDC`/`udc`, `LeAppStore`, `Lsf`, `TVT` | **NOT INVESTIGATED** — real contract work, see below |

## Detail

### `keyboard.backlight` — firmware absent

Both the data query and the method reject, so there is no read path to build a
write contract on:

```text
LENOVO_LIGHTING_DATA (query, no WHERE)        -> 不支持
LENOVO_LIGHTING_DATA WHERE Lighting_Id = 0    -> ROOT\WMI query: WMI HRESULT 0x8004100C
LENOVO_LIGHTING_METHOD.Get_Lighting_Current_Status(Lighting_ID 0..3) -> 不支持
```

The legacy `Invoke-WmiMethod` route answers `Invalid method Parameter(s)`, which
is the class-invocation shape (no instance), not a firmware verdict; the
instance-based route is the one that returns `不支持`. `LENOVO_GAMEZONE_DATA`
also declares `GetKeyboardLight`/`IsSupportLightingFeature`/`SetKeyboardLight`,
and those return `无效的对象` like the rest of the non-`IsSupport*` group.

### `fan.mode` — firmware absent, with one caveat

`GetSmartFanMode` rejects, and the fan-table plane rejects:

```text
LENOVO_GAMEZONE_DATA.GetSmartFanMode   -> 无效的对象
LENOVO_FAN_METHOD.Fan_Get_Table(FanID=1, SensorID=0) -> 不支持
```

The registry does carry `ITS_FanMode=4`, and the dispatcher's strings show it
reads and writes that value (for example
`[BIOS]::SetFullSpeedMode,bSetITSV %d,REG_ITS_CurrentSetting %d`). That makes
`ITS_FanMode` a **dispatcher-internal projection**, not an authoritative
user-mode control surface: the dispatcher owns it, reconciles it with ITS mode,
and nothing documents the stable meaning of `4`. Reporting it as fan mode would
present a cache as a channel, so it is not wired up.

### `bios.save` — deliberate boundary, not a gap

The HAL implements `save()`. `bios set` calls it internally, so a real setting
change is staged, saved, and read back. The standalone `bios save` subcommand
refuses on purpose: calling `SaveBiosSettings` on its own would commit whatever
is in the BIOS staging buffer, which belongs to the firmware and may hold
changes this CLI never staged (a previous interrupted writer, the vendor tool,
or firmware defaults). The CLI cannot describe or roll back that content, so it
will not commit it.

The refusal is correct; only its wording was opaque. It now states the reason.

### `update.*` — a real vendor stack exists, no contract recovered

The machine carries Lenovo's own updater components (registry subtrees
`DriverUpdate`, `SystemUpdateAddin`, `UDC`/`udc`, `LeAppStore`, `Lsf`, `TVT`
under `HKLM\SOFTWARE\Lenovo` and the WOW6432Node mirror). That means an update
channel exists in principle. What is not established is a contract this CLI may
implement against: the catalog endpoint, its signature/authentication chain,
the schema, and the applicability rules. `docs/09-linux-backend.md` and the
`update` capability reasons already refuse to guess.

Recovering that contract is a distinct work item (endpoint + signature +
schema + applicability), not a name fix. It is left explicitly unimplemented
rather than approximated against an unauthenticated endpoint.

## What this changes

- No new capability was enabled by this sweep. The sweep's value is negative
  knowledge: five channels that looked like they might be stale-name misreads
  are not, and each now has a recorded reason.
- One wording fix: `bios.save` explains why it refuses.

## Scope

Read-only. No `Set*` WMI method was invoked, including `SaveBiosSettings` /
`DiscardBiosSettings`; no registry value was written; no network request was
made.

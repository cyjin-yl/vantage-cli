# POWER-AC — adapter / USB-C PD / battery power draw — recon + contract — 2026-11-05

Scope: read-only `power ac` and `power draw` on the Windows target
`tb14p-ezra` (ThinkBook 21VG). Question: **which read-only channels on this
SKU actually expose adapter input power and instantaneous battery draw, what exactly
does each channel report, and where must the command refuse to state a number?**

No hardware state was changed. All target probes were schema queries, CIM reads, and two
read-only EnergyDrv queries (`GBMD 0xFF`, and GAPD only if its documented gate were set;
it was not). No write IOCTL was issued, no charge mode or power scheme was altered.

## 1. Verdict

| requested value | channel on this SKU | status |
|---|---|---|
| AC line state | `GetSystemPowerStatus` (`ACLineStatus`) | **available**, live confirmed |
| adapter identity / certification | `GBMD 0xFF` word (bits 15–16), via EnergyDrv | **available**, live confirmed `0x00860004` |
| adapter instantaneous output power | `GAPD 0x8310215c` 10-byte reply, gated by GBMD bit 24 | **not advertised** on this target (bit 24 = 0), so no read |
| adapter rated wattage | no channel | **unavailable** — GAPD has no rated field; never inferred |
| adapter voltage / current | no channel | **unavailable** — GAPD exposes PID/VID/SystemPowerW/CurrentPowerW only |
| USB-C PD contract (V, A, W) | UCM-UCSI device exists; no documented user-mode API | **unavailable** — no read implemented |
| battery-pack instantaneous rate (draw/charge) | SetupAPI battery class `IOCTL_BATTERY_QUERY_STATUS` → `BATTERY_STATUS.Rate` | **available** (signed mW; relative rates withheld) |
| battery-pack terminal voltage | same `BATTERY_STATUS.Voltage` | **available** (mV) |
| wall-input / whole-system input power | no sensor, no documented API | **unavailable** — never derived from adapter or battery numbers |

`Win32_Battery` on this SKU has **no `Voltage` and no `PowerRate` property**; it cannot be
used for draw and the implementation does not fall back to it.

## 2. Live target observation (read-only)

Observed 2026-11-05, `18:32:44Z`–`19:03:50Z`, machine `21VG`, on AC.

### 2.1 AC state and battery idle state

| probe | result |
|---|---|
| `CallNtPowerInformation(SystemBatteryState)` | return 0, struct size 32: `AcOnLine=1`, `BatteryPresent=1`, `Charging=0`, `Discharging=0`, `MaxCapacity=93060`, `RemainingCapacity=73990`, `Rate=0`, `EstimatedTime=0xFFFFFFFF`, `DefaultAlert1=1861`, `DefaultAlert2=9306` |
| `root\wmi:BatteryStatus` | `Active=true`, `PowerOnline=true`, `Charging=false`, `Discharging=false`, `Critical=false`, `RemainingCapacity=73990`, `Voltage=16833`, `ChargeRate=0`, `DischargeRate=0`, `Tag=10` |
| `Win32_Battery` (1 instance) | `BatteryStatus=2`, `EstimatedChargeRemaining=79`, `DesignVoltage=16833`, `Chemistry=6`, `Status=OK`, `DesignCapacity=null`, `FullChargeCapacity=null` |

Two independent rate observations therefore agree on a **measured zero** while on AC:
the battery is neither charging nor discharging. That is not a system-input measurement;
it is a zero pack flow.

### 2.2 Properties that do not exist (schema, not values)

`Win32_Battery` property list queried from the class itself (33 names):
`Caption, Description, InstallDate, Name, Status, Availability, ConfigManagerErrorCode,
ConfigManagerUserConfig, CreationClassName, DeviceID, ErrorCleared, ErrorDescription,
LastErrorCode, PNPDeviceID, PowerManagementCapabilities, PowerManagementSupported,
StatusInfo, SystemCreationClassName, SystemName, BatteryStatus, Chemistry, DesignCapacity,
DesignVoltage, EstimatedChargeRemaining, EstimatedRunTime, ExpectedLife,
FullChargeCapacity, MaxRechargeTime, SmartBatteryVersion, TimeOnBattery, TimeToFullCharge,
BatteryRechargeTime, ExpectedBatteryLife`.
→ **no `Voltage`, no `PowerRate`.** The only voltage-like field is `DesignVoltage`, which
is a design value, not a terminal measurement.

`root\wmi:BatteryStatus` property list (observed):
`Caption, Description, Name, Frequency_Object, Frequency_PerfTime, Frequency_Sys100NS,
Timestamp_Object, Timestamp_PerfTime, Timestamp_Sys100NS, Active, ChargeRate, Charging,
Critical, DischargeRate, Discharging, InstanceName, PowerOnline, RemainingCapacity, Tag,
Voltage`.
→ it has `Voltage`, `ChargeRate`, `DischargeRate`, but **no `Rate`** on this build.

Other probe classes:

| probe | result |
|---|---|
| `Win32_VoltageProbe` | 1 instance, `Name="Numeric Sensor"`, `CurrentReading/Nominal/Min/Max = null`, `Status="Unknown"`, `DeviceID="root\cimv2 0"` → placeholder, not a usable source |
| `Win32_CurrentProbe` | 0 instances |
| PnP `Battery` class | 2 devices: `Microsoft ACPI-Compliant Control Method Battery`, `Microsoft AC Adapter`; both `Status=OK` |
| UCM / USB Type-C related | `UCM-UCSI ACPI Device` (class `UCM`, `Status=OK`), `USB4(TM) Host Router (Microsoft)` (`Status=OK`) — device presence only |

## 3. Channel contracts

### 3.1 `GBMD 0xFF` + `GAPD 0x8310215c` (vendor adapter contract)

Contract already established in `docs/02-power-battery.md` §3.2 and §8, and re-confirmed
live here:

- `IOCTL 0x831020f8` with the one-byte subcommand `0xFF` returns a 4-byte DWORD.
  - bits 15–16: adapter type (`0=Inbox`, `1=Lenovo`, `2=Unknown`, `3=SlowCharger`)
  - bit 24: detailed-adapter capability
- Only when bit 24 = 1 may the implementation issue `IOCTL 0x8310215c` (GAPD), input
  4 zero bytes, output 10 bytes `{u16 PID, u16 VID, u16 SystemPowerW, u16 CurrentPowerW}`;
  PID/VID `0xFFFF` means "absent", and a zero power value is the documented unknown
  value (`GetAdapterWattage`, W, ≤0 = unknown), so the projection keeps it `null`.
- `CurrentPowerW < SystemPowerW` is the underpower condition. `CurrentPowerW` is the
  **instantaneous adapter output the firmware reports**, not a rated wattage.

Live observation at `19:03:50Z`, AC connected:

```
CreateFileW("\\.\EnergyDrv", GENERIC_READ, SHARE_READ|WRITE, OPEN_EXISTING) -> ok
GBMD 0xFF -> 0x00860004  (bit 24 = 0)
```

- `0x00860004` is the same baseline recorded in `docs/02-power-battery.md` §3.2
  ("无认证充电器, Inbox 类型").
- Because bit 24 = 0, **GAPD was not called**; this is the contract, not an error.
- Consequence: on this target the adapter object can report `has_detail=false`,
  `detail=null`, `wattage_w=null`, `ac_connected=null`, `status=null`. The live AC state
  comes from `GetSystemPowerStatus`, not from a fabricated GAPD value.

`0x831020f4` (the other GAPD-family code named in `docs/01-hal-interfaces.md` §3.2) has no
recovered output layout in this tree and is not called; no adapter voltage/current
reading is claimed from it.

### 3.2 SetupAPI battery class — `BATTERY_STATUS`

Documented contract (Microsoft Learn, `BATTERY_STATUS` structure, Poclass.h):

| member | meaning |
|---|---|
| `PowerState` | bitmask: `0x1` POWER_ON_LINE, `0x2` DISCHARGING, `0x4` CHARGING, `0x8` CRITICAL |
| `Capacity` | current capacity, mWh (or relative) |
| `Voltage` | **current battery voltage across the terminals, mV**; `BATTERY_UNKNOWN_VOLTAGE` when unavailable |
| `Rate` | **current charge/discharge rate**; mW, or arbitrary units/hour when `BATTERY_CAPACITY_RELATIVE` is set; **positive = charging, negative = discharging**; `BATTERY_UNKNOWN_RATE` when unavailable |

Existing implementation: `crates/lctrl-hal-win/src/native_battery_electrical.rs`
enumerates `GUID_DEVCLASS_BATTERY`, admits one non-short-term system battery
(`collect()` at line 245), queries tag/information (`0x0029_4040`/`0x0029_4044`), then
`BATTERY_QUERY_STATUS` (`0x0029_404c`, line 364). `project()` (line 88) already:
- copies `PowerState` raw,
- maps `0xFFFFFFFF` voltage to `None`,
- **withholds** the rate when `BATTERY_CAPACITY_RELATIVE` is set or the value is the
  `BATTERY_UNKNOWN_RATE` sentinel (`i32::MIN`).

This battery-class IOCTL was not issued in the present probe (the CLI acceptance run does
that); the AC-idle zero in §2.1 comes from the two independent WMI/kernel observations.
Prior on-battery evidence
(`docs/evidence/target-battery-source-separation-20261028.json`) recorded
`rate_mw=-19151`, `voltage_mv=16053` from this channel and `discharge_rate_raw=19151`
from WMI in the same window — the signed-IOCTL vs. unsigned-WMI-magnitude convention.

### 3.3 `SYSTEM_BATTERY_STATE` (kernel aggregate)

Documented (winnt.h): `Rate` is the current battery rate in mW; positive charging,
negative discharging, treated as `LONG`. Live call returned a 32-byte structure with
`Rate=0` on AC. It is an aggregate battery figure and is **not** a wall-input sensor.

This source is **not implemented as a second draw channel** in this change: adding it
would extend `BatteryControl`/`OsBatteryChannel` and introduce a second attribution path
for one number. It is recorded here only as live corroboration and as the AC-line
cross-check. `power draw` reports the battery-class channel and names it.

### 3.4 USB-C PD

The UCM-UCSI ACPI device and the USB4 host router are present and healthy, but Windows
does not expose the negotiated PD contract (voltage/current/wattage) through a documented
user-mode API, and this tree has no recovered vendor channel for it. `power ac` therefore
reports PD values as unavailable with that reason. No value is derived from adapter
identity, adapter name, or a nominal rating.

## 4. Implementation contract (read-only)

### 4.1 `power ac`

Sources, each kept separate (never merged into one synthetic adapter):

| output field | source | rule |
|---|---|---|
| `ac_line_status` | `GetSystemPowerStatus.ACLineStatus` | `true/false`; `null` when the API does not report it |
| `ac_line_source` | literal | the exact channel named by the platform composition root |
| `adapter` | EnergyDrv `GBMD 0xFF` + gated GAPD | full `AdapterInfo`; `null` on channel error with `adapter_error` |
| `adapter_source` | literal | the exact channel asked for the adapter observation |
| `adapter_detail_advertised` | GBMD bit 24 | `true/false`; `null` when no adapter observation |
| `adapter_current_power_w` | GAPD `CurrentPowerW` | only when GAPD was read; `null` otherwise |
| `adapter_system_power_w` | GAPD `SystemPowerW` | only when GAPD was read; `null` otherwise |
| `unavailable[]` | fixed reasons | adapter voltage, adapter current, rated wattage, PD contract; plus "detail not advertised" when bit 24 = 0 |
| `source_issues[]` | per-source failures | e.g. `ACLineStatus` not reported |

The command fails only when both the AC-line source and the vendor adapter channel fail;
a partial observation succeeds with the failed source still present as an error object.
On Linux the same report uses the `power_supply` `online` bit (reached through the
adapter channel) because `GetSystemPowerStatus` is Windows-only; `ac_line_source` and
`adapter_source` always name the channel that was actually asked.

### 4.2 `power draw`

| output field | source | rule |
|---|---|---|
| `rate_mw` | `BATTERY_STATUS.Rate` | signed, verbatim; `null` when unknown/relative |
| `voltage_mv` | `BATTERY_STATUS.Voltage` | mV; `null` when unknown sentinel |
| `power_state_bits` | `BATTERY_STATUS.PowerState` | raw bitmask |
| `state` | derived only from `Rate` | `discharging` / `charging` / `zero` / `not_reported` |
| `draw_mw` | derived from `Rate < 0` | `abs(rate)`; `null` in every other state |
| `charge_mw` | derived from `Rate > 0` | `rate`; `null` in every other state |
| `ac_line_source` | literal | the exact channel named by the platform composition root |
| `source` | literal | `SetupAPI battery class IOCTL_BATTERY_QUERY_STATUS (BATTERY_STATUS)` |
| `unavailable[]` | fixed reasons | wall-input power, adapter output power, adapter voltage/current |

Honesty rules enforced in `lctrl-core`:

1. `rate_mw = Some(0)` is reported as **`state="zero"` with `draw_mw=null`**, never as
   `0 W` draw and never silently as "not reported".
2. `rate_mw = None` is `state="not_reported"`; no draw is computed.
3. On AC with a zero rate, an issue states that the platform has no wall-input or
   adapter-output power meter, so system input power is not asserted.
4. No current (`mA`) is derived from `rate / voltage`; `rate` is power, not a measured
   current, and the existing channel contract already refuses that conversion.
5. Human output converts mW to W only as a unit rendering of the measured milliwatts.

## 5. Non-claims

- Not a wall-power meter. `power draw` is battery-pack electrical flow.
- Not a rated-adapter check. `CurrentPowerW` is instantaneous and only present when the
  firmware advertises detail; a missing GAPD reply is not "0 W adapter".
- No USB-C PD negotiation data, no UCSI property read, no ACPI `_DSM` probing.
- No write, no charge-mode change, no power-scheme change, no service or driver mutation.
- `Win32_Battery`, `Win32_VoltageProbe`, and `Win32_CurrentProbe` are not used as power
  sources on this SKU.

## 6. Evidence provenance

- Target `tb14p-ezra`, `Win32_ComputerSystem.Model = 21VG`, 2026-11-05.
- CIM schema/instance reads: `Win32_Battery` (class + 1 instance), `root\wmi:BatteryStatus`
  (class + 1 instance), `BatteryStaticData`/`BatteryFullChargedCapacity`/`BatteryCycleCount`
  class presence, `Win32_VoltageProbe`, `Win32_CurrentProbe`, `Get-PnpDevice -Class Battery`,
  UCM/USB4 friendly names.
- `CallNtPowerInformation(SystemBatteryState)` P/Invoke, 32-byte structure, return 0.
- Read-only EnergyDrv queries: `GBMD 0xFF` (`0x00860004`); GAPD intentionally not issued
  because bit 24 = 0.
- Prior on-battery evidence reused for the discharging sign/magnitude:
  `docs/evidence/target-battery-source-separation-20261028.json`.
- Contracts: Microsoft Learn `BATTERY_STATUS` (Poclass.h) and `SYSTEM_BATTERY_STATE`
  (winnt.h); repo `docs/02-power-battery.md` §3.2/§8 and `docs/01-hal-interfaces.md` §3.2.

Not run here (parent acceptance): building the binary, running `vantage power ac`,
`vantage power draw`, or any unit/contract test. A source edit is not verified behavior.

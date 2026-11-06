# Battery health: wear, cycle count, design vs full-charge capacity — owner, units, implementation — 2026-11-05

Scope: `battery health` (read-only). Resolves which channel on the 21VG target actually
exposes design capacity, full-charge capacity and cycle count, fixes the unit semantics
(mWh versus mAh), and fixes the refusal rules for wear percentage. Recon on the target was
read-only: PowerShell WMI/CIM class and instance queries only. No IOCTL was issued, no
program was built or run, no registry/service/hardware state changed.

## 1. Owner resolution (three candidates, one winner)

| candidate | what it exposes on this target | verdict |
|---|---|---|
| `Win32_Battery` (root\CIMV2) | `DesignCapacity = null`, `FullChargeCapacity = null` (both 2026-11-05 samples); only `Name=L25D4PE3`, `DeviceID=660SunwodaL25D4PE3`, `EstimatedChargeRemaining`, `BatteryStatus`, `Chemistry=6`, `EstimatedRunTime=71582788` (`0x04444444`, not a runtime). | **not an owner**: the class schema has the properties, the provider publishes none |
| `root\WMI` `Lenovo_BatteryInformation` | 6 instances `ACPI\PNP0C14\WBAT_0..5`; the class declares exactly `{Active, CurrentSetting, InstanceName}` (`Get-WmiObject -List` property dump). `CurrentSetting` values are identity strings: `BAT0 BatMaker,0001`, `BAT0 HwId    ,0002`, `BAT0 MfgDate ,XXXXXSSSYYY` and the `BAT1` trio. | **not an owner**: identity strings only — no capacity, no cycle count, no health code |
| `root\WMI` battery class chain (`MSBatteryClass` family) | `BatteryStaticData.DesignedCapacity`, `.Capabilities`, `.DeviceName`, `.ManufactureName`, `.Chemistry`, `.Technology`, `.DefaultAlert1/2`, `.CriticalBias`, `.Tag`; `BatteryFullChargedCapacity.FullChargedCapacity`; `BatteryCycleCount.CycleCount`; `BatteryStatus.{RemainingCapacity,Voltage,PowerOnline,Charging,Discharging,Critical,ChargeRate,DischargeRate}`; `BatteryRuntime.EstimatedRuntime` | **owner** for design capacity, full-charge capacity, cycle count |
| `\\.\EnergyDrv` `0x83102138` (via `PowerBattery.dll`) | 83-byte response, verified layout offsets `0x00/0x02/0x04` = design/full/remaining in units of 10 mWh (docs/01 §3.3, docs/02 §7.1) | **corroborating** channel: it agreed exactly with WMI on 2026-10-28 (99900 / 94090 / 65240 mWh). It has no cycle counter: cycles would need `0x83102120` (SMBus firmware + cycle count), which is not implemented |

Consequence for the CLI: `battery health` takes its capacities from the `root\WMI` battery
class chain (one identity-matched pack, `InstanceName` association already enforced in
`lctrl-hal-win/src/os_battery.rs`), reports the vendor firmware health code and life
percentage beside it, and never merges the two channels into one number.

### 1.1 Provider access method (why the binding matters)

`Get-CimInstance` (MI/CIM) cannot read the class that carries the design capacity:

```
Get-CimInstance -Namespace root/WMI -ClassName BatteryStaticData
  -> 常规故障 / HRESULT 0x80041001 (generic failure)      2026-11-05, repeated
Get-CimInstance -Namespace root/WMI -ClassName MSBatteryClass
  -> 常规故障 / HRESULT 0x80041001                        2026-11-05
Get-CimInstance -Namespace root/WMI -ClassName BatteryFullChargedCapacity -> ok
Get-WmiObject   -Namespace root/WMI -Class BatteryStaticData -> DesignedCapacity = 99900
```

So the design capacity is reachable only through classic WMI (`IWbemLocator`/
`IWbemServices::ExecQuery`). The repository's `WmiTransport` is the classic COM binding and
`OsBatteryChannel` already reads these classes through it; a future move to MI would silently
lose the design capacity and therefore the wear percentage.

## 2. Unit semantics (exact, not inferred from names)

Source: MSDN `BATTERY_INFORMATION` (`Poclass.h` / `Batclass.h`), the structure the WMI battery
class chain exposes.

| field | documented unit / rule |
|---|---|
| `Capabilities` bit `0x4000_0000` (`BATTERY_CAPACITY_RELATIVE`) | set ⇒ "the battery capacity and rate information are relative, and not in any specific units … all rate information is reported in units per hour". Clear ⇒ **mWh for capacity and mW for rate**. |
| `DesignedCapacity` | "the theoretical capacity of the battery when new, in mWh unless `BATTERY_CAPACITY_RELATIVE` is set. In that case, the units are undefined." |
| `FullChargedCapacity` | "the battery's current fully charged capacity in mWh (or relative). **Compare this value to `DesignedCapacity` to estimate the battery's wear.**" |
| `CycleCount` | "the number of charge/discharge cycles the battery has experienced … **If the battery does not support a cycle counter, this member is zero.**" |
| `DefaultAlert1/2`, `CriticalBias` | low/warning alert levels and a bias, also in mWh — independent corroboration that the class is in mWh on this target (`DefaultAlert2 = 9381` ≈ 10 % of a 99.9 Wh pack, not 9.4 Wh of an unrelated scale) |

Target observation: `Capabilities = 2147483648 = 0x8000_0000`, i.e. `BATTERY_SYSTEM_BATTERY`
only — `BATTERY_CAPACITY_RELATIVE` **is not set**, so every capacity and rate on this pack is
absolute mWh / mW. Verified twice (2026-10-28 and 2026-11-05).

**No mAh is ever derived.** A mWh→mAh conversion needs the terminal voltage at the same
instant: the only voltage the vendor structure exposes at a fixed offset (`0x14` = 15600 mV) is
*nominal/design* voltage, and `BatteryStatus.Voltage` is instantaneous (15956 mV while
discharging and 16797 mV while charging, minutes apart). Dividing a capacity by either one
would publish a fabricated ampere-hour figure. Capacities therefore keep the `_mwh` suffix and
no `_mah` field exists.

## 3. Live target readings (2026-11-05T16:49:13Z, read-only)

| record | field | value |
|---|---|---|
| `BatteryStaticData` | `DesignedCapacity` | **99900 mWh** |
| | `Capabilities` | 2147483648 (`0x8000_0000`) |
| | `DeviceName` | L25D4PE3 |
| | `ManufactureName` | Sunwoda |
| | `Chemistry` | 1313818956 = `0x4E4F494C` = `"LION"` (little-endian ASCII) |
| | `Technology` | 1 (rechargeable) |
| | `DefaultAlert1` / `DefaultAlert2` / `CriticalBias` | 1876 / 9381 / 0 mWh |
| | `Tag` | 9 |
| `BatteryFullChargedCapacity` | `FullChargedCapacity` | **93810 mWh** |
| `BatteryCycleCount` | `CycleCount` | **233** |
| `BatteryStatus` | `RemainingCapacity` | 60440 mWh |
| | `Voltage` | 15956 mV |
| | `PowerOnline` / `Charging` / `Discharging` | false / false / true |
| | `ChargeRate` / `DischargeRate` | 0 / 14583 mW |

Derived (floor arithmetic, exactly what the implementation computes):

- `capacity_ratio_percent = floor(100 × 93810 / 99900) = 93`
- `wear_percent = floor(100 × (99900 − 93810) / 99900) = floor(6.09) = 6`

They need not sum to 100; both are floored and documented as such. An earlier 2026-10-28
sample of the same pack read design 99900 / full 94090 / cycle 225 (ratio 94), which is the
same scale with a slightly different full-charge estimate — the design capacity is stable,
the full-charge estimate tracks the pack, and the cycle counter advances (`225 → 233`).

Corroboration of the capacity scale across channels (2026-10-28, retained receipt
`target-battery-source-separation-20261028.json`): WMI and the `\\.\EnergyDrv` device
response independently reported the same 99900 / 94090 / 65240 mWh triple.

## 4. Refusal rules (the part that must not be guessed)

1. **Design capacity absent or 0** ⇒ `capacity_ratio_percent = null`, `wear_percent = null`.
   The raw full-charge capacity is still reported, and an issue names the missing design
   capacity. MSDN's own "0 = unsupported" encoding is never read as a measured zero, and
   never divides.
2. **Full-charge capacity absent or 0** ⇒ both percentages `null`, same rule.
3. **Full-charge capacity above design capacity** ⇒ the ratio stays observable (it is a real
   reading, and can exceed 100), but `wear_percent = null` with an issue: a negative wear
   figure would be a fabricated claim about a pack the firmware mis-reports.
4. **`BATTERY_CAPACITY_RELATIVE` set** ⇒ relative units are undefined; capacities are
   withheld by the channel (it records "relative capacity units; mWh unavailable") and no
   percentage is computed.
5. **`CycleCount == 0`** ⇒ `cycle_count = null` plus a source issue. This is new behaviour:
   the class documents 0 as "no cycle counter", so a fabricated "0 cycles" would be a false
   claim about a worn pack. A non-zero count is reported verbatim.
6. **Vendor firmware health** (`BatteryHealth` 1..5, docs/02 §7.2) and `life_percent` come
   from the EnergyDrv device response and are reported as `vendor_health` with their own
   source label. On this target both are `null` today (the 83-byte parser deliberately does
   not decode the unverified tail), so health cannot be claimed from the vendor channel.

## 5. Implementation map

| change | file / symbol | reason |
|---|---|---|
| `BatteryFieldProvenance { design_capacity, full_charge_capacity, cycle_count }`, `BatteryWearReport { source, fields, design_capacity_mwh, full_charge_capacity_mwh, cycle_count, capacity_ratio_percent, wear_percent, issues, has_observation() }` | `crates/lctrl-core/src/battery.rs`, re-exported in `crates/lctrl-core/src/lib.rs` | one arithmetic home for the wear rule; every raw number keeps the record that produced it |
| `BatteryControl::battery_wear()` (default `Unsupported { feature: "battery.health" }`, forwarded by the `&T` impl) | `crates/lctrl-hal/src/battery.rs` | the HAL owns transport and provenance; the CLI only renders |
| `WindowsBatterySources::battery_wear()` → `root_wmi_battery_classes` with `root\WMI:BatteryStaticData.DesignedCapacity`, `root\WMI:BatteryFullChargedCapacity.FullChargedCapacity`, `root\WMI:BatteryCycleCount.CycleCount` | `crates/lctrl-hal-win/src/battery_sources.rs` | reuses the identity-matched snapshot instead of opening a second, unassociated query |
| `CycleCount == 0` ⇒ `None` + issue | `crates/lctrl-hal-win/src/os_battery.rs` | MSDN encoding, see §4.5 |
| `LinuxHal::battery_wear()` → label per actual node (`energy_full_design` vs `charge_full_design` converted with voltage) | `crates/lctrl-hal-linux/src/lib.rs` | same contract on sysfs; provenance names the node that supplied the number |
| `capabilities.record("battery.health", …)` | `crates/lctrl-hal-win/src/windows_hal.rs`, `crates/lctrl-hal-linux/src/lib.rs` | capability matrix honesty |
| `CommandId::BATTERY_HEALTH` + registry row (read-only, Runtime layer, `Capability("battery.health")`, service `Battery`, deps: optional EnergyDriver, optional PublicWin32Cim, required LinuxSysfs) + `BatteryCommand::Health` + dispatch | `crates/lctrl-core/src/command_capability.rs`, `crates/lctrl-hal/src/command_capability.rs`, `crates/vantage-cli/src/lib.rs`, `crates/vantage-cli/src/capability.rs` | every command needs one registry row and one layer |
| `battery_health::execute` | `crates/vantage-cli/src/battery_health.rs` (new) | emits `source_association: "unverified"`, `capacity_wear`, `capacity_wear_error`, `vendor_health` (`source: "energy_drv_83102138_device0"`, `health`, `life_percent`), `vendor_health_error`; fails only when the capacity channel itself failed |

Authority and lifecycle: read-only. No write, no service, no registry, no persistence, no
restore step; nothing survives the process.

## 6. Acceptance the parent must run (not run here)

1. `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc` (cross), then on the target
   `vantage --json battery health` → expect `capacity_wear.design_capacity_mwh = 99900`,
   `full_charge_capacity_mwh` near 93810, `cycle_count` 233-ish, `wear_percent` 6-ish,
   `fields.*` naming the three `root\WMI` records, `vendor_health.health = null`.
2. Cross-check the same numbers against `Get-WmiObject root\WMI BatteryStaticData` /
   `BatteryFullChargedCapacity` / `BatteryCycleCount` in the same minute.
3. Negative controls:
   - a fake channel with `design = None` and `full = Some(x)` ⇒ `wear_percent: null`,
     `capacity_ratio_percent: null`, `full_charge_capacity_mwh: x`, issue mentions the design
     capacity (covered by `battery_health::tests::missing_design_capacity_refuses_wear_but_keeps_the_observation`);
   - `CycleCount = 0` row ⇒ `cycle_count: null` + `BatteryCycleCount` issue (needs a new
     `windows_os_battery` case; not added here because the same file is being edited
     concurrently);
   - `full > design` ⇒ ratio > 100 reported, `wear_percent: null`.
4. `vantage capability` must list `battery.health` as Available on this target and must not
   claim it on a machine where the battery class chain is absent.
5. `battery status` must be unchanged (`os_battery.cycle_count` keeps its previous numeric
   value; only the zero case now becomes null with an issue).

## 7. Remaining unknowns (explicit)

- Vendor firmware health code / `life_percent` are `null` on this target: the values exist in
  the EnergyDrv response tail only under offsets still marked [INFERENCE] in docs/02 §7.1, and
  `SmartBatteryInfo` V2 (`0x8310214b`, `0x8310213c`) remains unproven on 21VG. No health
  classification is claimed from them.
- `0x83102120` (SMBus firmware + cycle count) is not implemented, so there is no second,
  independent cycle-count reading to cross-check the WMI counter against.
- `Win32_Battery.Chemistry = 6` is not mapped to the `"LION"` string here; the WMI class
  chemistry is the one reported.
- The battery's PnP instance path and its serial number are omitted from this document on
  purpose; the `WBAT_0..5` ACPI method instances in §1 are the documented vendor class shape,
  not pack identity.

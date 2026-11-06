# Thermal sensors and DPTF participant readback — 2026-11-05

Target: `tb14p-ezra` (ThinkBook 21VG, Panther Lake). All probes read-only, run
over SSH with Windows PowerShell (`Get-CimInstance` / `Get-PnpDevice` /
`Get-PnpDeviceProperty` / `findstr`). No write path is exercised in this
document.

Scratch transcripts on the target (not committed):
`C:\Users\ezra\thermal-probe1.txt` … `thermal-probe9.txt`.

| probe | content |
|---|---|
| 1 | `MSAcpi_ThermalZoneTemperature`, `Win32_TemperatureProbe`, `Win32_PerfFormattedData_Counters_ThermalZoneInformation`, `root/WMI` class census, `LENOVO_*`/`Lenovo_*` instance dump |
| 2 | property/method signatures and instances for `BatteryTemperature`, `EsifDeviceInformation`, `KernelThermal*`, `LENOVO_SMART_THERMAL_MONITOR_EVENT`, `LENOVO_FAN_*`, `LENOVO_GAMEZONE_DATA`; driver/service/PnP census |
| 3 | participant device identity (`PCI\VEN_8086&DEV_B01D`), ACPI participant properties, two 10 s-apart ESIF snapshots, simultaneous ACPI/perf-counter read |
| 4 | GAMEZONE invocation errors, DTT driver-store file list, DTT registry, participant `DEVPKEY_*` |
| 5 | native `System.Management` invocation attempt, 4×15 s ESIF series, DTT data dirs, `dtt_sw.inf` |
| 6 | class/property qualifiers (unit declarations) for `EsifDeviceInformation` and `MSAcpi_ThermalZoneTemperature` |
| 7 | `findstr` for the unit string and participant-name strings across the DTT driver store, event-log check |
| 8 | `PowerBattery.dll` search, `ACPI\PNP0C14\*` device list, cimv2 sensor class instance counts, TZ00 device properties |
| 9 | the exact WQL projections the HAL issues, plus a simultaneous ACPI/ESIF/perf-counter snapshot |

## 1. Source inventory on this SKU

| source | interface | instances | returns a temperature | verdict |
|---|---|---|---|---|
| ACPI firmware zone | `root\WMI MSAcpi_ThermalZoneTemperature` | 1 (`ACPI\ThermalZone\TZ00_0`) | yes, tenths of Kelvin | **used** |
| Intel DTT/IPF participants | `root\WMI EsifDeviceInformation` | 16 (same device path, `_0`…`_15`) | 8 of 16 readings; 8 report 0 | **used** (see §3) |
| Battery pack | Windows battery class IOCTL `BATTERY_QUERY_INFORMATION`/`Temperature` | 1 | yes, tenths of Kelvin | **used** (existing channel) |
| `Win32_TemperatureProbe` (`root\cimv2`) | CIM | 1 placeholder | no (`CurrentReading` empty) | rejected, §4.1 |
| `root\WMI BatteryTemperature` | CIM | 0 | class registered, no instance | rejected, §4.2 |
| `LENOVO_GAMEZONE_DATA.GetCPUTemp/GetGPUTemp` | WMI-ACPI | 1 instance, methods absent | no | dead on this SKU, §4.3 |
| `LENOVO_FAN_TABLE_DATA` `Sensor_ID` | WMI-ACPI | none (`WBEM_E_NOT_SUPPORTED`) | no | dead on this SKU, §4.4 |
| `LENOVO_SMART_THERMAL_MONITOR_EVENT` | WMI event | 0 standing instances | no (event only) | not a sensor source |
| HID sensor collections (`HID\VID_8087&PID_0AC2`, ×2) | Sensor API only | – | not exposed through CIM/WMI | not enumerable here |
| `Win32_PerfFormattedData_Counters_ThermalZoneInformation` | cimv2 perf counter | 1 (`\_TZ.TZ00`) | mirrors the ACPI zone | rejected as a *second* sensor, §4.5 |

## 2. `MSAcpi_ThermalZoneTemperature` — one firmware zone

Live rows (deci-Kelvin as returned; °C = value/10 − 273.15):

| probe | `InstanceName` | `CurrentTemperature` | `CriticalTripPoint` | `ActiveTripPoint` | count | `PassiveTripPoint` | `ThermalStamp` |
|---|---|---|---|---|---|---|---|
| 1 | `ACPI\ThermalZone\TZ00_0` | 3162 (43.05 °C) | 3782 (105.05 °C) | 3732, 3282, 3232, 3182, 3132 | 5 | 0 | 96 |
| 3 | `ACPI\ThermalZone\TZ00_0` | 3242 (51.05 °C) | 3782 | 3732, 3282, 3232, 3182, 3132 | 5 | 0 | 101 |
| 8 | `ACPI\ThermalZone\TZ00_0` | 3182 (45.05 °C) | 3782 | – | 5 | 0 | – |
| 9 | `ACPI\ThermalZone\TZ00_0` | 3182 (45.05 °C) | 3782 | – | 5 | 0 | – |

Device identity: PnP `ACPI\THERMALZONE\TZ00`, `DeviceDesc = "ACPI Thermal Zone"`,
HardwareIds `ACPI\ThermalZone *ThermalZone`,
`LocationPaths = ACPI(_SB_)#ACPI(TZ00)`. `ThermalStamp` increases between reads,
so the zone is live rather than a frozen firmware value.

Nothing in the public class names the physical site of `TZ00`. It is therefore
reported with `location = unknown`; the HAL never relabels it as `CPU`/`GPU`.
The ACPI zone read (45.05 °C at probe 9) and the ESIF processor-participant read
(71 at the same moment) differ by ≈26 °C, which is itself evidence that the zone
is *not* the CPU package sensor — if it were, both readings would have to agree.

## 3. `EsifDeviceInformation` — the real DPTF/IPF participant readback

### 3.1 Class and instance identity

* Namespace `root\WMI`, 16 instances, `Active = True` for all.
* Method surface: `EsifExecutePrimitive(inputData: UInt8Array, instanceId: UInt16,
  methodId: UInt16)` — the primitive plane is **not wired** on this build: every
  `methodId`/`instanceId`/`inputData` combination returns "invalid parameter"
  (`docs/01-hal-interfaces.md`, note dated 2026-11-05). Participant *names* are
  therefore not obtainable from this channel.
* Every instance `InstanceName` shares one device path and differs only in the
  trailing index:

```
PCI\VEN_8086&DEV_B01D&SUBSYS_380617AA&REV_04\3&11583659&0&20_0 … _15
```

  That device is a real, working PnP node (probe 4):

```
PNP OK class=System  name="Intel(R) Innovation Platform Framework Processor Participant"
DEVPKEY_Device_LocationPaths = PCIROOT(0)#PCI(0400) ACPI(_SB_)#ACPI(PC00)#ACPI(TCPU)
DEVPKEY_Device_LocationInfo = PCI bus 0, device 4, function 0
```

  Its ACPI counterparts exist as separate participants under the IPF manager:
  `ACPI\INTC10D4\IETM` children
  = `ACPI\INTC10D5\CHRG`, `ACPI\INTC10D8\TPWR`, `ACPI\INTC10D5\SEN1..SEN5`.
  The WMI class does **not** carry any of those ACPI instance names, and the
  index suffix (`_0`…`_15`) cannot be mapped to them from anything the provider
  publishes. The HAL therefore keys each sensor by the provider's own instance
  name and never by position.

### 3.2 Property set and declared units (probe 6, class qualifiers)

| property | declared description |
|---|---|
| `Temperature` | "Participant Temperature Value In 1/10 K" |
| `Power` | "Participant RAPL Power Value In .1 Watts" |
| `Ac0TripPoint` / `Ac1TripPoint` | "Active 0/1 trip point value in 1/10 K" (`_AC0`/`_AC1`) |
| `CrtTripPoint` / `PsvTripPoint` / `HotTripPoint` / `Aux0/1TripPoint` | "…trip point value in 1/10 K" |

### 3.3 Readings

Snapshot A/B are 10 s apart (probe 3), C–F are 15 s apart (probe 5), G is probe 9.
Column = instance suffix; `T/P` = `Temperature`/`Power` as returned.

| suffix | A | B | C | D | E | F | G |
|---|---|---|---|---|---|---|---|
| 0 | 69/173 | 66/171 | 66/106 | 63/113 | 74/170 | 65/147 | 71 |
| 1 | 69/119 | 66/112 | 66/72 | 63/78 | 74/128 | 66/107 | 71 |
| 2 | 69/4 | 66/8 | 66/3 | 63/2 | 74/4 | 66/4 | 71 |
| 3 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0 |
| 4 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0 |
| 5 | 55/0 | 55/0 | 51/0 | 50/0 | 51/0 | 51/0 | 50 |
| 6 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0 |
| 7 | 32/0 | 32/0 | 26/0 | 26/0 | 26/0 | 27/0 | 27 |
| 8 | 47/0 | 47/0 | 36/0 | 36/0 | 36/0 | 36/0 | 35 |
| 9 | 52/0 | 52/0 | 38/0 | 38/0 | 38/0 | 38/0 | 37 |
| 10 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0 |
| 11 | 59/55 | 60/55 | 51/55 | 51/55 | 51/55 | 51/55 | 52 |
| 12–15 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0/0 | 0 |

Trip points were identical on all 16 instances in every snapshot:
`Ac0TripPoint = 67`, `Ac1TripPoint = 55`, `Ac2..Ac9 = 0`,
`CrtTripPoint = 0`, `PsvTripPoint = 0`, `HotTripPoint = 0`, `Aux0/1 = 0`.
They are per-participant **limits**, not measurements, and are not surfaced as
sensors.

### 3.4 Unit of `Temperature` — declared "1/10 K", observed Celsius

The class metadata says 1/10 K, but that wording is boilerplate: it is the same
sentence used for the trip points, and it is attached to a class whose
`Power` really is a tenths-scaled quantity (228 → 22.8 W, 55 → 5.5 W). Three
independent checks show the *temperature* field is not tenths-scaled:

1. `Temperature = 66` as tenths of Kelvin is **6.6 °C**. In the same second the
   ACPI firmware zone `TZ00` read 45.05 °C and the battery pack read 28.75 °C
   (tenths-Kelvin from the battery class). A live SoC participant cannot be
   colder than the battery and the board zone.
2. `Ac0TripPoint = 67` / `Ac1TripPoint = 55` are standard active-trip
   temperatures (67 °C / 55 °C). As tenths of Kelvin they would be 6.7 °C /
   5.5 °C, which is not a meaningful trip point for any part of this machine.
3. The three processor-side instances (`_0`,`_1`,`_2`) move together between
   snapshots (69/69/69 → 66/66/66 → 74/74/74 → 71/71/71) while their `Power`
   values differ, i.e. the field tracks die temperature, not a scaled copy of
   something else.

**Conclusion [INFERENCE, labelled]:** on this platform the provider publishes
`Temperature` in whole degrees Celsius. The HAL converts nothing and reports the
value as °C. If a future BIOS/`ipf` build honours the documented 1/10 K scale,
the reported values would jump to ≈3000 and become physically absurd; the parent
acceptance below includes that sanity check.

### 3.5 Zero readings are absence, not 0 °C

Eight of the sixteen instances (`_3`, `_4`, `_6`, `_10`, `_12`–`_15`) returned
`Temperature = 0` in **every** snapshot (7 snapshots over ≈2 minutes) while their
peers moved, and they carry `Power = 0` as well. Reporting those as `0.0 °C`
would publish a measurement the provider never made. The HAL therefore emits no
sensor for them: only instances that return a temperature are surfaced, and a
`0` is never published as a measured temperature. The count is recorded here so
the omission stays auditable (16 instances enumerated, 8 readings). A reply that
carries a row *without* a `Temperature` property, or with a non-numeric value, is
a malformed response and aborts the read instead of being skipped.

## 4. Sources examined and rejected

### 4.1 `Win32_TemperatureProbe`

One instance, but it is the CIM placeholder, not a sensor (probe 1):

```
Name = "Numeric Sensor"  Status = Unknown  CurrentReading = <empty>
Accuracy = 32768  MaxReadable = 32768  MinReadable = 32768  Resolution = 32768
Tolerance = 32768  NominalReading = <empty>  PNPDeviceID = <empty>
```

`32768` = `0x8000` is the "unknown" filler for every numeric field, and
Microsoft's `Win32_TemperatureProbe` reference
(<https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-temperatureprobe>)
describes the class as generally unpopulated by WMI providers. No sensor is
emitted for it.

### 4.2 `root\WMI BatteryTemperature`

The class is registered (`InstanceName`, `Tag`, `Temperature`), but the query
returned **0 instances** (probes 2, 3). The working battery temperature is the
Windows battery-class IOCTL value already used by the HAL
(`windows-battery-class/temperature`, e.g. 3019 deci-K = 28.75 °C in
`docs/evidence/target-read-baseline-20261027.json`), not this class.

### 4.3 `LENOVO_GAMEZONE_DATA` temperature getters

`GetCPUTemp`, `GetGPUTemp`, `GetCpuFrequency`, `GetThermalMode`,
`GetSmartFanMode`, `IsSupportSmartFan` … are declared on the single active
instance `ACPI\PNP0C14\GMZN_0`, but this firmware has no GameZone method bodies:
every one of them returns `WBEM_E_INVALID_OBJECT` (0x8004100F) while
`IsSupportGpuOC` answers, i.e. the provider is reachable and the rejection is
per method (`docs/evidence/wmi-capability-exhaustion-20261104.md` §4;
`docs/03-thermal-performance.md` errata of 2026-08-27).

Probes 6/7 attempted the same calls through the CIM cmdlet path and
`System.Management`; both failed *client-side* ("invalid method parameters" /
"Operation is not valid due to the current state of the object"), which is the
same cmdlet binding artefact the fan recon hit
(`docs/evidence/re-fan-write-20261105.md` §1). Those two client failures are
**not** the evidence — the SWbem HRESULT table above is. No CPU/GPU package
temperature is claimed from this channel.

### 4.4 Fan-table sensor enumeration

`LENOVO_FAN_TABLE_DATA`, `LENOVO_FAN_METHOD` and `LENOVO_FAN_MAX_SPEED_DATA`
exist as classes but every instance query returns
`WBEM_E_NOT_SUPPORTED` ("不支持", probe 2). The documented
`LENOVO_FAN_TABLE_DATA.Sensor_ID` enumeration therefore yields nothing on this
SKU, and no fan-coupled sensor is emitted. (`LENOVO_FAN_TEST_DATA` does answer:
`NumOfFans = 1` — that is fan inventory, not temperature.)

### 4.5 Thermal-zone perf counter

`Win32_PerfFormattedData_Counters_ThermalZoneInformation` reports exactly one
zone, `\_TZ.TZ00`, next to the WMI class's `ACPI\ThermalZone\TZ00_0`:

| probe | WMI `CurrentTemperature` | counter `HighPrecisionTemperature` |
|---|---|---|
| 3 | 3242 (51.05 °C) | 3282 (55.05 °C) |
| 9 | 3182 (45.05 °C) | 3232 (50.05 °C) |

Same zone, different sampling instants; the counter is a second *view*, not a
second sensor. Emitting it would duplicate `TZ00` under a different name, so it
is not surfaced.

### 4.6 Event-only and out-of-band surfaces

`LENOVO_SMART_THERMAL_MONITOR_EVENT` (`Status: UInt32`),
`KernelThermalConstraintChange` (`ThermalConstraint`, `Processors`) and
`KernelThermalPolicyChange` are event classes with no standing instances; they
describe throttling state, not a temperature. The two Intel HID sensor
collections (`HID\VID_8087&PID_0AC2`) are reachable only through the Windows
Sensor API — no `root\WMI`/`root\cimv2` class instance exposes them, so they are
not enumerable by this HAL.

## 5. `LENOVO_REPORT_DBDC_DATA`

Live row (probe 1, instance `ACPI\PNP0C14\GMZN_0`):

```
Active = True            Counts = 3
CurrentLimit = {7500, 5000, 4500}     (mA)
ROS_Power    = {0, 0, 0}              (mW)
Threshold    = {100, 40, 20}          (battery %, high → low)
```

All three arrays agree with `Counts = 3`, so the existing
`DbdcProfile::is_consistent` check passes and `perf dbdc` reports the row
verbatim. `ROS_Power` is **0 for every tier**: the provider declares no
per-tier power ceiling here. That is a limit that is absent, not a measured
0 mW — consumers must not present it as a power measurement.

## 6. EC temperature through `\\.\EnergyDrv`

The verified EnergyDrv surface in this codebase is: GBMD status/write,
`GAPD` adapter detail, battery-config (20 B), battery detail (83 B), EM spec
version (`BSIF`), USB-charger flags, night-charge, temporary-charge. Every one is
charge/adapter related; **none of them is a temperature operation**, and no
temperature-bearing field is claimed from the 83-byte battery-detail buffer.

The battery temperature that `perf temp` reports does exist, but it comes from
the Windows battery class (`IOCTL_BATTERY_QUERY_INFORMATION` /
`BATTERY_QUERY_INFORMATION` level `Temperature`, tenths of Kelvin), which the HAL
already reads — not from the Lenovo EC.

`PowerBattery.dll` (the EM library that fronts `\\.\EnergyDrv`) was not present
under `C:\Windows\System32`, `C:\Windows\SysWOW64`,
`C:\Program Files\Lenovo` or `C:\Program Files (x86)\Lenovo` on this machine
(probe 8), so no export scan was possible there either. Absence of evidence is
reported as absence: **no EC temperature channel is claimed or implemented**, and
`perf temp` does not advertise one.

## 7. What the HAL now emits (`perf temp`)

| id | name | source | location | availability |
|---|---|---|---|---|
| `acpi/<InstanceName>` | `ACPI thermal zone (<InstanceName>)` | `acpi` | `unknown` | available (tenths-K → °C) |
| `esif/<InstanceName>` | `Intel ESIF participant (<InstanceName>)` | `wmi_esif` | `unknown` | available; participants that report 0 are not surfaced at all (§3.5) |
| `windows-battery-class/temperature` | `Windows battery class temperature (unassociated)` | `windows_battery_class` | `battery` | available / unavailable (unchanged) |

Sorting is by id, duplicate ids are rejected, and an entirely empty source set
still yields `Unsupported { feature: "perf.temp" }`. Query errors other than
"class absent" (service, permission, malformed row) are preserved rather than
swallowed — unchanged from the existing behaviour.

The capability probe behind `perf.temp` / `channel.temperature` now treats the
ESIF readback as a third *readable* source: a machine whose ACPI zone is absent
but whose participants answer is reported `available`, and `perf.temp`'s detail
string lists which sources were readable next to the failures. An absent class
is not a failure; a query error or a malformed row is.

The two new WQL projections were validated live on the target before use
(probe 9):

```
SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature  -> 1 row
SELECT InstanceName, Temperature FROM EsifDeviceInformation                  -> 16 rows
```

## 8. Inference register

| claim | status |
|---|---|
| ESIF `Temperature` unit is °C on this platform | **[INFERENCE]** from the three cross-checks in §3.4; the MOF declares 1/10 K |
| ESIF instance suffix identifies a participant, not a physical site | observed (device path identical, suffix differs); no name is published |
| Zero ESIF readings mean "no temperature available for this participant" | **[INFERENCE]** from 8 instances reading 0 across 7 snapshots while 8 others move; the provider has no explicit absence marker |
| `TZ00` is not the CPU package sensor | supported by the 26 °C gap to the ESIF processor participant at the same instant; physical site still unknown |
| `Ac0TripPoint = 67` is 67 °C | same unit argument as §3.4; not independently read back |

## 9. Parent acceptance and negative controls

Behavioural checks for the parent (not run here; this worker does not build):

1. `vantage perf temp` on `tb14p-ezra` lists **10** sensors, not 18: 1 ACPI zone
   + 1 battery + 8 ESIF participants with readings. The 8 zero-valued
   participants are absent from the list (never `0.0 °C`).
2. No sensor is named `CPU` or `GPU`. Every ESIF name contains its own
   `PCI\VEN_8086&DEV_B01D…_N` instance string.
3. `vantage perf temp read "esif/…_3"` (a zero-valued participant) fails with
   "sensor was not found", and `--json` contains no `0.0` ESIF value.
4. `--json` output has `"source": "wmi_esif"` for the new rows and unchanged
   values for the ACPI/battery rows.
5. Negative control: on a machine without `EsifDeviceInformation` (or with the
   class query failing as `Unsupported`), `perf temp` must return only the
   ACPI/battery rows — the ESIF path must not turn absence into an empty error.
6. Negative control: `Win32_TemperatureProbe` must never appear; if a future
   build starts reporting it, that is a new source needing its own evidence.
7. Negative control: `perf dbdc` must keep printing `ROS_Power = 0` verbatim for
   this SKU (do not "fix" it to `unknown` without a provider-level absence proof).
8. Sanity on the unit inference: read `perf temp` twice under different load;
   ESIF values must stay in the 20–100 range and move roughly with the ACPI
   zone. Values near 3000 would mean the 1/10 K declaration is honoured and the
   conversion in this document is wrong.

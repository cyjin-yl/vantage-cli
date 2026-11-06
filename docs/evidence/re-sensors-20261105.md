# Sensors on the 21VG target: what exists, what does not, and the recovered read contract — 2026-11-05

Scope: issue item `SENSORS: ambient light / accelerometer / hall / tablet-mode`.
Question answered here: **which sensors does this SKU actually expose**, through which owning
component, and what a bounded read-only `sensor list` / `sensor read` may claim.

Machine: `TB14P-EZRA` (ThinkBook 21VG), Windows 11, live session 2026-11-05/06.
All probes below are read-only: no sensor was opened for writing, no report interval was
changed, no vendor service was started or stopped.

## 1. Verdict

| question | answer (observed) |
|---|---|
| ambient light | **present**: `CM32183 ALS Sensor` (Capella CM32183), Windows category `LIGHT`, type `AMBIENT_LIGHT`, `SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX` = `VT_R4` |
| accelerometer / gyro / compass / inclinometer / orientation / barometer | **absent**: every WinRT `GetDefault()` returns `null`; the Sensor API inventory contains no sensor of those types |
| hall / lid-close sensor | **no hall sensor device**. The lid is the ordinary ACPI lid switch `ACPI\PNP0C0D`, not a sensor-class instance |
| tablet mode / slate | **absent**: no `ACPI\PNP0C60` (display-sensor device), no convertible/slate-mode device, no WinRT `SimpleOrientationSensor` |
| vendor WMI sensor surface | **none**: `root\WMI` has 1650 classes, no `LENOVO_*SENSOR*`/ALS/accelerometer class |
| ESIF | **no device**: `\\.\ESIF` does not exist; only the IPTF `EsifDeviceInformation`/`EsifWmiEvent` classes exist (event/description only, no sensor data) |

The single sensor channel that returns data is the **public Win32 Sensor API**
(`SensorsApi.dll`, `CLSID_SensorManager {77A1C827-FCD2-4689-8915-9D613CC5FA3E}`), served by the
Intel ISS HID sensor collection (UMDF). That is the only channel this implementation uses.

Live inventory is **9 sensors**, not 1: one is a standard ambient-light sensor and the other
**eight are `SENSOR_TYPE_CUSTOM` vendor sensors in category `OTHER`** (AMS TMF882x time-of-flight
presence/gesture/HOD chains and Intel keyboard-lighting channels). They are listed with their
real identity and their real data keys; their vendor `CUSTOM_VALUE*` fields are never
interpreted as light, tilt, or presence by this CLI.

## 2. Probes (exact)

Evidence files on the target (`C:\Users\ezra\evidence\`):

| file | content |
|---|---|
| `sensors-20261105.txt` | PnP class `Sensor`, ACPI namespace sweep, `root\WMI` class inventory (1650 names), WinRT sensor defaults, sensor services/drivers, per-class instance reads |
| `sensors2-20261105.txt` | sensor device-interface enumeration (`GUID_DEVINTERFACE_SENSOR`), HID collection descriptors/reads, HID instance properties, parent devices, sensor COM registration |
| `sensors3-20261105.txt` | WinRT `LightSensor` live readings, ESIF/IPTF driver inventory, HID instance registry parameters |
| `sensors5-oracle-20261105.txt` | Win32 Sensor API oracle: `CoCreateInstance`, `GetSensorsByCategory(ALL)`, per-sensor identity/state/properties, `GetData` |
| `sensors6-fields-20261105.txt` | per-sensor `GetSupportedDataFields` (via `IPortableDeviceKeyCollection`) and every supported value with its `VARTYPE`, two rounds 1.5 s apart |

Key invocations (PowerShell, `-File`, no encoded commands):

```
Get-PnpDevice -PresentOnly | ? Class -eq 'Sensor'
Get-CimClass -Namespace root/WMI                      # 1650 classes, filtered offline
[Windows.Devices.Sensors.LightSensor, Windows.Devices.Sensors, ContentType=WindowsRuntime]::GetDefault().GetCurrentReading()
# SetupAPI: SetupDiGetClassDevs({97f115c8-599a-4153-8894-d2d12899918a}) -> device interfaces
# COM: CoInitializeEx(APARTMENTTHREADED); CoCreateInstance(CLSID_SensorManager, IID_ISensorManager)
#      ISensorManager::GetSensorsByCategory(SENSOR_CATEGORY_ALL)
```

## 3. Live inventory (Sensor API, `SENSOR_CATEGORY_ALL`, 2026-11-05T18:53Z)

`GetCount` = 9 on both rounds. Identity columns are what the driver publishes (no synthesis):
`friendly name`, `manufacturer`, `model`, `serial_number` (the driver's `LUID:` encoding),
`device_path` (the device-interface instance, including its reference-string GUID), category,
type, `SENSOR_PROPERTY_HID_USAGE`, `MIN_REPORT_INTERVAL`, and the supported data-field count.

| # | friendly name | mfr / model | category | type | HID usage | min interval (ms) | data fields | `GetData` |
|---|---|---|---|---|---|---|---|---|
| 0 | `AMS_TMF882X V30056 Sensor` | AMS / TMF882x | `OTHER` `{2c90e7a9-…}` | `CUSTOM` `{e83af229-…}` | 34050 | 200 | 29 | report, values |
| 1 | `Keyboard Lighting Mode` | INTEL / INTEL_GPIO | `OTHER` | `CUSTOM` | 617 | 10 | 5 | report, values |
| 2 | `Dynamic KB Lighting` | INTEL / DKB Lighting | `OTHER` | `CUSTOM` | 777 | 10 | 5 | report, values |
| 3 | `AMS_HUMAN_DETECT V3010 Sensor` | AMS / TMF882x | `OTHER` | `CUSTOM` | 550 | 200 | 7 | report, values |
| 4 | `AMS_HPD_GR` | AMS / TMF882x_HP_GR | `OTHER` | `CUSTOM` | 34051 | 200 | 21 | report, values |
| **5** | **`CM32183 ALS Sensor`** | **CAPELLA / CM32183** | **`LIGHT` `{17a665c0-…}`** | **`AMBIENT_LIGHT` `{97f115c8-…}`** | *not published* (`0x80070490`) | **250** | 5 | **report incl. `light_level_lux`** |
| 6 | `Lenovo AMS_GESTRUE V0209 Sensor` | LENOVO / AMS_HPDSEETING | `OTHER` | `CUSTOM` | 34487 | 10 | 5 | report, values |
| 7 | `Lenovo AMS_HPD V0302 Sensor` | LENOVO / Lenovo AMS_HPD | `OTHER` | `CUSTOM` | 34452 | 10 | 5 | **`S_OK` with a null report** |
| 8 | `AMS_TMF882X HOD V300 Sensor` | AMS / TMF882x_HOD | `OTHER` | `CUSTOM` | 34052 | 200 | 10 | **`S_OK` with a null report** |

`GetSensorsByType(SENSOR_TYPE_AMBIENT_LIGHT)` returns exactly **1** sensor (row 5), so the ALS
is the only standard-typed sensor on this machine.

Row 5 device path (verbatim; the reference string is the sensor's identity on the instance):

```
\\?\HID#Vid_8087&Pid_0AC2#7&3c430d9&0&0000#{c317c286-c468-4288-9975-d4c4587c442c}\{00410002-7DB7-0002-0000-000000000000}
```

Rows 0, 3, 4, 5, 7, 8 hang off HID collection `HID\VID_8087&PID_0AC2\7&3C430D9&0&0000`
(`\Device\000000c0`, `UINumber` 0); rows 2 and 6 hang off the second collection
`HID\VID_8087&PID_0AC2\7&36B1C06D&0&0000` (`\Device\000000c1`, `UINumber` 1). Both collections
report `DEVPKEY_Device_HardwareIds = HID\VID_8087&UP:0020_U:0001` (HID sensors page) and are
driven by `WUDFRd` with `{5175d334-c371-4806-b3ba-71fd53c9258d}\0000` / `…\0001` (the Sensor
class driver), behind `IshHidMini` on `HECI_BUS`. Raw HID access is closed to user mode on both
collections: `CreateFile` succeeds but `HidD_GetPreparsedData` and
`IOCTL_HID_GET_REPORT_DESCRIPTOR` fail with Win32 1168 (`ERROR_NOT_FOUND`). The Sensor API is
therefore the only read path, which is what this implementation uses.

### 3.1 The ambient-light reading

```
GetState            -> SENSOR_STATE_READY (0)
GetData             -> S_OK
GetTimestamp        -> SYSTEMTIME (UTC, ms resolution)
GetSensorValue({E4C77CE2-DCB7-46E9-8439-4FEC548833A6}, pid 2 = SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX)
                    -> VT_R4
SupportsDataField(LIGHT_LEVEL_LUX)      -> VARIANT_TRUE
SupportsDataField(LIGHT_TEMPERATURE_KELVIN / LIGHT_CHROMACITY) -> VARIANT_FALSE
GetProperty(LIGHT_LEVEL_LUX)            -> 0x80070490 (ERROR_NOT_FOUND)   # data lives in the report
```

Observed values were **0 lx** on every sample; the room was dark and the ALS is under the
display bezel. A zero lux value is a valid reading and is reported as such — it is **not** an
error and **not** "no data". Data-vs-none is decided by the report timestamp and the sensor's
`MIN_REPORT_INTERVAL` (250 ms), not by the value:

* `report_timestamp_utc`, `age_ms = now - timestamp`, `fresh = -1000 <= age_ms <= max(2000, 4*min_interval)`.
* Row 6 (`Lenovo AMS_GESTRUE`) publishes a report timestamp **2026-11-07T17:57Z** while the wall
  clock was 2026-11-05T18:53Z, i.e. ~47 h in the future. Such a report is listed, its timestamp
  is surfaced, and `fresh=false`. It is never clamped to "now" and never silently dropped.
* Rows 7 and 8 return `S_OK` with a **null** `ISensorDataReport`. That is "no data", not a zero
  reading; they are reported in the inventory's `unavailable` set.

## 4. Cross-check with the WinRT projection

Same sensor, same numbers, through `Windows.Devices.Sensors.LightSensor`:

```
GetDefault()          -> DeviceId
   \\?\HID#Vid_8087&Pid_0AC2#7&3c430d9&0&0000#{97f115c8-599a-4153-8894-d2d12899918a}\{00410002-7DB7-0002-0000-000000000000}
MinimumReportInterval -> 250     MaxBatchSize -> 512
GetCurrentReading()   -> lux=0 on 6 consecutive samples, timestamps advancing
GetDefault() for Accelerometer / SimpleOrientationSensor / OrientationSensor / Gyrometer /
Compass / Magnetometer / Inclinometer / Barometer / Altimeter -> null
ProximitySensor / Pedometer / ActivitySensor -> type has no GetDefault
```

Two independent projections (Win32 Sensor API and WinRT) agree on identity, interval, and value,
which is why the CLI reads through the Win32 API: it needs no WinRT activation, no
`IAsyncOperation` handling, and no package identity.

## 5. Negative evidence for the sensors that are not there

| claim | evidence |
|---|---|
| no ALS ACPI device | `ACPI\ACPI0008` **ABSENT** (explicit instance query, all states) |
| no tablet/slate-mode device | `ACPI\PNP0C60` **ABSENT**, `ACPI\PNP0C6A` **ABSENT**, `ACPI\ACPI0011` **ABSENT** |
| no accelerometer etc. | WinRT `GetDefault()` null for all 9 motion/orientation classes; `GetSensorsByCategory(ALL)` has 9 entries, of which the only standard type is `AMBIENT_LIGHT` |
| no vendor WMI sensor class | full `root\WMI` class dump (1650 names) contains no `LENOVO_*` sensor/ALS/hall class; sensor-adjacent names are `MSAcpi`, `MSAcpiInfo`, `MSAcpi_ThermalZoneTemperature`, `EsifDeviceInformation`, `EsifWmiEvent`, `SensorClassExtensionControlGuid`, `WiFi_GenericSensorData`, `WiFiSensorNotificationEvent`, plus `LENOVO_LIGHTING_*`/`LENOVO_SMART_THERMAL_MONITOR_EVENT` (documented elsewhere in this repo) |
| no ESIF sensor device | `\\.\ESIF`, `\\.\Esif` do not exist; the IPTF stack (`ipf_acpi`) owns only power/charging participants (`CHRG`, `TPWR`, `SEN1`…`SEN5`) under `ACPI\INTC10D4\IETM`; no sensor data class |
| sensor services are not the channel | `SensorService`, `SensorDataService`, `SensrSvc` are all **Stopped** while both the WinRT and Win32 reads succeed: the data is served by the UMDF sensor device, not by those services |
| lid is not a sensor | `ACPI\PNP0C0D\5&15D725F4&0` = "ACPI Lid" (class System); no `Sensor`-class instance and no sensor interface for it |

`ACPI\IDEA2002` ("Lenovo Intelligent Sensing", `WUDFRd`) exists but publishes no sensor
interface and no `\\.\` device node; SmartSense/HPD settings remain their own vendor RPC
channel (`sense`), documented separately. The AMS ToF sensors in rows 0/3/4/6/8 are the
*hardware* those settings act through; their custom values are not an HPD settings surface and
this CLI does not read them as presence state.

## 6. Recovered contract: `C:\WINDOWS\System32\SensorsApi.dll`

Pulled from the target for disassembly (public Windows API, not a vendor binary):

```
sha256  121d6f3d9a8c69d9db5dcbfe91d6e24441d6d81b7afa3e9c31cf4e92975d3b2d
PE32+ x86-64, image base 0x180000000, 25 exports
registration  HKLM\SOFTWARE\Classes\CLSID\{77A1C827-FCD2-4689-8915-9D613CC5FA3E}\InprocServer32
              = C:\WINDOWS\System32\SensorsApi.dll
```

Interface table in `.rdata` (RVA → IID), confirmed as the sensorsapi table by the typelib GUID
`{15092592-5270-4C6C-AAA0-11764F189A8F}` at RVA `0x56D40` matching the library uuid declared
next to these interfaces:

| RVA | GUID | identity |
|---|---|---|
| `0x56CA0` | `{23571E11-E545-4DD8-A337-B89BF44B10DF}` | `IID_ISensorCollection` |
| `0x56CB0` | `{BD77DB67-45A8-42DC-8D00-6DCF15F8377A}` | **`IID_ISensorManager`** |
| `0x56CC0` | `{D5FB0A7F-E74E-44F5-8E02-4806863A274F}` | internal (not used) |
| `0x56CD0` | `{5FA08F80-2657-458E-AF75-46F73FA6AC5C}` | `IID_ISensor` |
| `0x56CE0` | `{9B3B0B86-266A-4AAD-B21F-FDE5501001B7}` | `IID_ISensorManagerEvents` |
| `0x56CF0` | `{5D8DCC91-4641-47E7-B7C3-B74F48A6C391}` | `IID_ISensorEvents` |
| `0x56D00` | `{0AB9DF9B-C4B5-4796-8898-0470706A2E1D}` | `IID_ISensorDataReport` |
| `0x590B0` | `{77A1C827-FCD2-4689-8915-9D613CC5FA3E}` | `CLSID_SensorManager` |

Method order is the declaration order in the public IDL (`sensorsapi.idl`, `ISensorManager`,
`ISensorCollection`, `ISensor`, `ISensorDataReport`, and `IPortableDeviceKeyCollection` from
`portabledevicetypes.idl`). Slot indices below are vtable indices counting `IUnknown` as 0-2:

```
ISensorManager  3 GetSensorsByCategory   4 GetSensorsByType   5 GetSensorByID
                6 SetEventSink           7 RequestPermissions
ISensorCollection 3 GetAt   4 GetCount   5 Add  6 Remove  7 RemoveByID  8 Clear
ISensor         3 GetID   4 GetCategory  5 GetType   6 GetFriendlyName  7 GetProperty
                8 GetProperties  9 GetSupportedDataFields  10 SetProperties
                11 SupportsDataField  12 GetState  13 GetData  14 SupportsEvent
                15 GetEventInterest  16 SetEventInterest  17 SetEventSink
ISensorDataReport 3 GetTimestamp  4 GetSensorValue  5 GetSensorValues
IPortableDeviceKeyCollection 3 GetCount  4 GetAt  5 Add  6 Clear  7 RemoveAt
```

Every slot this implementation calls was exercised live before the Rust ABI was written; the
oracle output above records the HRESULT and the value for each call, so each slot index is
evidence-backed rather than guessed. A wrong slot would have produced a plausible-looking
`HRESULT` with a garbage out-parameter, which is why the oracle asserts *semantics*
(`GetType` must return `{97F115C8-…}` for row 5, `GetFriendlyName` must return the row's name,
`GetCount` must equal the number of enumerable entries, `MIN_REPORT_INTERVAL` must be a
plausible 10…250 ms) and not just `S_OK`.

### 6.1 Negative control (this is why the IID matters)

The mingw-w64 copy of `sensorsapi.idl` declares `uuid(bd7db67-…)` — seven hex digits in data1.
Using that transcription, `CoCreateInstance(CLSID_SensorManager)` returned **`E_NOINTERFACE`
`0x80004002`**. The GUID in the shipping `SensorsApi.dll` table is `{BD77DB67-45A8-42DC-8D00-6DCF15F8377A}`;
with it the same call returns `S_OK`. The binary is authoritative; the header text was wrong.

### 6.2 Data keys observed

| key | name | `VARTYPE` | sensors |
|---|---|---|---|
| `{E4C77CE2-DCB7-46E9-8439-4FEC548833A6}`,2 | `SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX` | `VT_R4` | row 5 only |
| `{DB5E0CF2-CF1F-4C18-B46C-D86011D62150}`,2 | `SENSOR_DATA_TYPE_TIMESTAMP` | `VT_FILETIME` | all 7 readable sensors |
| `{B14C764F-07CF-41E8-9D82-EBE3D0776A6F}`,5 | `SENSOR_DATA_TYPE_CUSTOM_USAGE` | `VT_UI4` | the 8 custom sensors |
| `{B14C764F-…}`,7…31 | `SENSOR_DATA_TYPE_CUSTOM_VALUE1…25` | `VT_UI4` | the 8 custom sensors |

`SENSOR_PROPERTY_*` (`{7F8383EC-D3EC-495C-A8CF-B8BBE85C2920}` with pid 2…24) are the identity
channel: `FRIENDLY_NAME` (9), `MANUFACTURER` (6), `MODEL` (7), `SERIAL_NUMBER` (8),
`DESCRIPTION` (10), `MIN_REPORT_INTERVAL` (12), `CURRENT_REPORT_INTERVAL` (13),
`DEVICE_PATH` (15), `PERSISTENT_UNIQUE_ID` (5), `HID_USAGE` (22). On this driver
`PERSISTENT_UNIQUE_ID` and `SENSOR_PROPERTY_TYPE` come back as `VT_CLSID` values that are not
real GUIDs (the vendor encodes its `LUID` there), and `ISensor::GetID` returns the same
`LUID`-derived bytes; therefore **identity is reported from `DEVICE_PATH` + `SERIAL_NUMBER` +
friendly name**, never from those two fields. `RANGE_MINIMUM`, `RANGE_MAXIMUM`, `RESOLUTION`
return `0x80070490` (`ERROR_NOT_FOUND`) on all 9 sensors and are reported as absent rather than
defaulted.

### 6.3 Where the fail-closed IID re-query stops

Every interface the implementation receives from `CoCreateInstance`,
`ISensorManager::GetSensorsByCategory`, `ISensorCollection::GetAt` and `ISensor::GetData` is
re-queried for the IID that `SensorsApi.dll` publishes (§6 table) and is rejected with
`E_NOINTERFACE` when it does not match, so a mis-declared vtable slot cannot be read.

One interface on this path is **not** re-queried: the `IPortableDeviceKeyCollection` returned by
`ISensor::GetSupportedDataFields`. Its IID is published by `portabledevicetypes`, not by
`SensorsApi.dll`, so it is not in the recovered `.rdata` table, and no trustworthy value could be
recovered without the driver binary. Rather than guess a GUID, that path is pinned by:

* its two leading slots (`GetCount`, `GetAt`, checked for `HRESULT` and for a plausible
  `PROPERTYKEY` per entry) — this is exactly the oracle that produced §6.2 and the per-row field
  counts of §3 (5 for the ALS, 29 for `AMS_TMF882X V30056`, …), which the implementation reports
  as `data_field_count`;
* a null out-parameter being treated as "no fields", not as success.

If the IID is ever recovered from the driver, `Interface::adopt_as(&IID_IPORTABLEDEVICE_KEY_COLLECTION)`
closes this last gap; until then the limitation is stated rather than papered over.

## 7. Implementation (what this change adds)

* `crates/lctrl-core/src/sensor.rs` — `SensorGuid`, `SensorState`, `SensorValue`,
  `SensorField`, `SensorDescriptor`, `SensorReading`, `SensorInventory`, plus the UTC formatter
  and the freshness rule. Type/category/data-key names are a table copied from the Windows SDK
  (`SENSOR_TYPE_*`, `SENSOR_CATEGORY_*`, `SENSOR_DATA_TYPE_*`); unknown GUIDs are printed raw.
  A report timestamp field outside its range — including a day the month does not have — yields
  "no timestamp" (`None`) instead of a rolled-over date, so a bogus device clock cannot become a
  plausible-looking `fresh` reading.
* `crates/lctrl-hal/src/sensor.rs` — `SensorControl` trait: `list()` and `read(target)`.
* `crates/lctrl-hal-win/src/sensor.rs` — hand-written Sensor API ABI (the five vtables above),
  `CoCreateInstance(CLSID_SensorManager, IID_ISensorManager)` with `CoInitializeEx(APARTMENTTHREADED)`,
  and the identity/state/property/report reads. Any unexpected `HRESULT` fails closed to
  `ChannelUnavailable`/`Unsupported`; a null report is "no data", never a zero value.
* `vantage sensor list` / `vantage sensor read <target>` — read-only. `list` prints the
  sensors that returned a report, and a separate `unavailable` set for enumerated sensors whose
  `GetData` produced no report. `read` resolves `target` as list index → device path →
  friendly name → unique case-insensitive substring, and refuses ambiguous selectors.
* Wiring (all additive):
  * `lctrl-core::CommandId::SENSOR_LIST` (`sensor.list`) and `SENSOR_READ` (`sensor.read`), both in
    `CommandId::ALL`; `Dependency::Win32SensorApi` (`platform() == Windows`, component
    "Windows Sensor API (SensorsApi: sensor collection and data reports)").
  * `lctrl-hal::ServiceRole::Sensor` (`"sensor"`) and two `COMMAND_REGISTRY` rows:
    read-only, `FeatureBinding::Capability("sensor.list")`, `ServiceBinding::One(ServiceRole::Sensor)`,
    required `Win32SensorApi`, `DispatchSupport::Implemented`.
  * `crates/vantage-cli/src/lib.rs` — `Command::Sensor` with `SensorCommand::{List, Read { target }}`
    (bare `sensor` lists), `CommandServices::sensor` + `with_sensor`, `execute_sensor`, the human
    formatters, `command_feature`/`unsupported_reason` arms; `Command → CommandId` and
    `StateLayer::NotApplicable` in `crates/vantage-cli/src/capability.rs`;
    `role_channel(ServiceRole::Sensor) = "channel.sensor"`.
  * `crates/lctrl-hal-win/src/windows_hal.rs::capabilities()` — records `channel.sensor` and
    `sensor.list` from one live `SensorControl::list(&WindowsSensors)` call (the real enumeration;
    it never calls `read`), with the detail "`N` sensor report(s); `M` enumerated sensor(s)
    returned no report".
  * `vantage/src/main.rs` (Windows root) attaches `WindowsSensors::new()` unconditionally, because
    each call opens its own COM apartment.

The capability probe is the same enumeration a caller gets: it reads one report per sensor (the
only way the Sensor API answers "is this sensor delivering data"), and never takes a second sample
or a targeted `read`.

Explicitly **not** claimed: no light value is computed from anything but
`SENSOR_DATA_TYPE_LIGHT_LEVEL_LUX`; no tilt/orientation value is synthesized from an absent
accelerometer; the eight custom vendor sensors are listed by identity only and their
`CUSTOM_VALUE*` fields are printed as opaque numbers (with `VARTYPE`), never as lux, tilt,
distance, or presence.

Scope correction for the parent: an earlier summary of this slice said "exactly ONE sensor
exists". The two live rounds in §3 are the record — **9** sensors are enumerated, and the
single-sensor claim is true only for the *standard* types (`GetSensorsByType(AMBIENT_LIGHT)` = 1).
The implementation follows the enumeration, not the summary, so a reader of `sensor list` sees
the eight `SENSOR_TYPE_CUSTOM` vendor sensors with their own identity and data keys. Motion,
orientation, inclinometer, compass/magnetometer, gyro and barometer classes are still absent
(`GetDefault()` = `null`, §5), and `read` on any of them fails as absent.

## 8. Not yet run (parent/acceptance)

Cross-check only (authoring worker, Linux host, 2026-11-06): `cargo check -p lctrl-hal-win
--target x86_64-pc-windows-msvc` reaches this crate and reports **no diagnostic in
`crates/lctrl-hal-win/src/sensor.rs` or for the sensor block in `windows_hal.rs`**; the crate as a
whole does not link yet because sibling slices in the same worktree are mid-edit
(`security.rs`, `touchpad_caps.rs`, `usb_ports.rs`). No target program was executed, so no
reading below is observed behaviour yet.

The parent should:

1. After the sibling slices land, `cargo check -p lctrl-hal-win --target x86_64-pc-windows-msvc`,
   then build and run on the target.
2. `vantage --json sensor list` — expect `sensors` = 7 readable rows (0,1,2,3,4,5,6) and
   `unavailable` = 2 rows (7 `Lenovo AMS_HPD`, 8 `AMS_TMF882x HOD`), with the ALS row's
   `light_level_lux` = `0` (`VT_R4` → `real`) in a dark room and `fresh` true. Human output must
   print the report timestamp and `illuminance 0 lx (fresh reading)`; the two `unavailable` rows
   must print `no report: …` and no value at all.
3. `vantage --json sensor read ambient-light` — resolves to `CM32183 ALS Sensor` (substring
   selector); `sensor read 5` must select the same row by index. Negative controls:
   `sensor read accelerometer` must fail as absent (never a zero-valued reading) and
   `sensor read zzz` must fail as an invalid selector.
4. `vantage sensor list` twice ~1 s apart: report timestamps must advance for the readable
   sensors, and row 6 must remain `fresh=false` with its future timestamp printed unchanged (it
   is ~47 h ahead of the wall clock and is never clamped to "now").
5. `vantage --json capability` (or `doctor`) on the target: `sensor.list` and `sensor.read` must be
   present, `service_roles` must contain `sensor`, and the `channel.sensor` detail must agree with
   the row counts of step 2. This is the same probe the matrix evaluates, so a mismatch means the
   recorder and the verb disagree.
6. Negative control for the channel: with the sensor device disabled (or on a host without the
   Intel ISS sensor collection), `sensor list` must fail closed — the capability probe records
   `channel.sensor` as `unavailable` and the verb reports `ChannelUnavailable`/`Unsupported`,
   never an empty success.

## 9. Linux backend: recommended stub (not applied here)

`crates/lctrl-hal-linux` is owned by the Linux audit in this wave, so this slice did not add the
module. Until a Linux sensor plane is recovered and validated, the CLI answers `sensor` on Linux
with the explicit `Unsupported` written in `unsupported_reason`/`execute_sensor`
(`sensor.list: no sensor channel is attached on this platform…`). The one-file stub that matches
the existing `LinuxRadio` precedent is:

* file `crates/lctrl-hal-linux/src/sensor.rs`, plus `mod sensor;` and
  `pub use sensor::LinuxSensors;` in `crates/lctrl-hal-linux/src/lib.rs`;
* `#[derive(Clone, Copy, Debug, Default)] pub struct LinuxSensors;` with `pub const fn new()`;
* `impl lctrl_hal::SensorControl for LinuxSensors` returning
  `Err(LctrlError::Unsupported { feature: "sensor.list on Linux (no sensor data plane is bound in \
  this backend; /sys/bus/iio is not implemented or validated)".to_owned() })` from `list()` and the
  same text with `sensor.read` from `read(_target)`;
* attach it in the `#[cfg(target_os = "linux")] fn main()` of `vantage/src/main.rs` as
  `let sensor = lctrl_hal_linux::LinuxSensors::new();` … `.with_sensor(&sensor)`.

Why no sysfs implementation is proposed: `/sys/bus/iio/devices` node names alone do not carry the
`*_scale`/`*_offset` calibration or the lux semantics, so deriving a light value from them without
a validated per-device contract would claim a reading this repo has no evidence for — the same
reason `/sys/class/rfkill` is not bound for radios.

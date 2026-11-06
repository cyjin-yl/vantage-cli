# NVMe/SSD identity and health sources on 21VG — 2026-11-06

Target: `tb14p-ezra` = ThinkBook 14 G8+ IPH (`21VG`), Union Memory
`UMIS UPJYJ1TBMNV1QWY` 1 TB NVMe (PCI `1cc4`, one physical disk, disk 0).
Scope: read-only recovery of drive identity and health. **No write path of any
kind is exercised or implemented** — no SMART self-test trigger, no
sanitize/secure-erase, no firmware slot write, no power-mode change.

All probes ran over SSH with Windows PowerShell 5.1 driving `Get-CimInstance`
and a C# `Add-Type` shim over `CreateFileW`/`DeviceIoControl`. Scratch
transcripts on the target (not committed):
`C:\Users\ezra\storage-probe1.txt` … `storage-probe7-r.txt`
(probe 4/5/6/7 ran under `runas /trustlevel:0x20000` to place the same code in
a restricted token — see §7).

| probe | content |
|---|---|
| 1 | `root\Microsoft\Windows\Storage` class census, `MSFT_PhysicalDisk`/`MSFT_StorageReliabilityCounter` instances + property metadata, `MSFT_Disk`, `Win32_DiskDrive`, `root\wmi` SMART classes, Storage cmdlet availability, disk/controller PnP devices |
| 2 | elevation of the SSH token, `Get-PhysicalDisk`/`Get-StorageReliabilityCounter`, association walk, first pass of `IOCTL_STORAGE_QUERY_PROPERTY` device-descriptor + NVMe SMART log page |
| 3 | `whoami /groups`, device descriptor at desired-access 0 vs `GENERIC_READ`, NVMe SMART log page (props 49 and 50), NVMe Identify Controller, temperature descriptors (51/52), management status (56), adapter property (1), seek-penalty (7) |
| 4 | restricted token: Storage-namespace WMI reads, `Get-StorageReliabilityCounter`, descriptor/log-page/temperature/management status at desired-access 0 vs `GENERIC_READ` |
| 5 | restricted token: `root\cimv2` `Win32_DiskDrive`/`Win32_ComputerSystem`, Identify Controller + Identify Namespace through the storage protocol, `IOCTL_DISK_GET_LENGTH_INFO`, `IOCTL_STORAGE_READ_CAPACITY`, `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` |
| 6 | restricted token: `CM_Get_Device_Interface_ListW(GUID_DEVINTERFACE_DISK)` enumeration, per-path `IOCTL_STORAGE_GET_DEVICE_NUMBER`/geometry/descriptor/identify, `\\.\PHYSICALDRIVE0..3` fallback |
| 7 | restricted token: byte-exact `DISK_GEOMETRY_EX` dump to pin the `DiskSize` offset |

## 1. Source inventory on this SKU

| source | interface | read requires admin? | usable fields | verdict |
|---|---|---|---|---|
| `\\.\PHYSICALDRIVEn` + `IOCTL_STORAGE_QUERY_PROPERTY` (device descriptor, protocol-specific NVMe, temperature, management status) | Win32 IOCTL, handle opened with **desired access 0** | **no** (works in a restricted token) | bus type, product/rev/serial, model/SN/FW from Identify Controller, NVMe SMART/Health log page, 3 temperatures + warn/crit thresholds, health status | **used — sole source** |
| `\\.\PHYSICALDRIVEn` + `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` | Win32 IOCTL, desired access 0 | no | absolute drive size (bytes), logical sector size, media type | **used** for capacity |
| `root\Microsoft\Windows\Storage` `MSFT_PhysicalDisk` | Storage WMI provider | **yes** | FriendlyName/Model, SerialNumber, MediaType, BusType, Size, FirmwareVersion, HealthStatus, PhysicalLocation | rejected as a runtime dependency (§6.1), kept as a cross-check |
| `root\Microsoft\Windows\Storage` `MSFT_StorageReliabilityCounter` | Storage WMI provider | **yes** | `Temperature`, `TemperatureMax`, latency maxima only; `Wear` and `PowerOnHours` are **empty/zero** here | rejected (§6.2) |
| `root\wmi` `MSStorageDriver_FailurePredict{Status,Data,Thresholds}` | WMI | — | class registered but **no instance** (`不支持`) | rejected (§6.3) |
| `Win32_DiskDrive` (`root\cimv2`) | CIM | denied in a restricted token | Model/SerialNumber/Size/Status | rejected as runtime dependency (§6.1) |
| `\\.\EnergyDrv` (`AcpiVpc.sys`) | Lenovo vendor IOCTL | n/a | no storage-health verb exists: the full dispatch table is SALS/HALS/GBMD/ITHC/GBID/SMTF/MHQI/KBLC/KBLR/KBLA/BTMC/BSIF/BNMC (docs/interfaces.md §11.4) | not a storage health source |
| vendor `storelib.dll` / `smartmontools.zip` (shipped inside PCManager/Vantage) | third-party binaries | — | would duplicate §1 row 1 | not used (docs/evidence/re-scan-20261105.md) |

## 2. Identity and capacity without elevation

Enumerating the disks needs no WMI: `CM_Get_Device_Interface_ListW` on
`GUID_DEVINTERFACE_DISK` `{53f56307-b6bf-11d0-94f2-00a0c91efb8b}` returned
exactly one interface path in a restricted token (probe 6):

```
\\?\SCSI#Disk&Ven_NVMe&Prod_UMIS_UPJYJ1TBMNV#5&29353f84&0&000000#{53f56307-b6bf-11d0-94f2-00a0c91efb8b}
```

Opening that path (or `\\.\PHYSICALDRIVE0`) with `dwDesiredAccess = 0` and
issuing `IOCTL_STORAGE_GET_DEVICE_NUMBER` (`0x2D1080`) gives
`DeviceType=7` (`FILE_DEVICE_DISK`), `DeviceNumber=0`, `PartitionNumber=0`, so
the PnP path and the `PHYSICALDRIVEn` ordinal are bound by the device, not by
list order. `\\.\PHYSICALDRIVE1..3` returned `ERROR_FILE_NOT_FOUND` (2), so the
ordinal sweep is bounded by absence, not by a timeout.

`STORAGE_DEVICE_DESCRIPTOR` (`IOCTL_STORAGE_QUERY_PROPERTY`, `PropertyId = 0`,
`QueryType = 0`), 396 bytes returned:

| offset | field | live value |
|---|---|---|
| 0 | Version | 40 |
| 4 | Size | 396 |
| 8 | DeviceType | 0 (`FILE_DEVICE_DISK`) |
| 10 | RemovableMedia | 0 |
| 12 | VendorIdOffset | 0 (**absent** — NVMe has no SCSI vendor id) |
| 16 | ProductIdOffset | 57 → `"UMIS UPJYJ1TBMNV"` (truncated at 16 bytes) |
| 20 | ProductRevisionOffset | 122 → `"1.0L"` (truncated at 4 bytes) |
| 24 | SerialNumberOffset | 139 → `"5CDF_B806_1090_9BA5."` |
| 28 | BusType | **17 = `BusTypeNvme`** |

The descriptor's product/revision fields are SCSI-shaped and truncated, so the
full strings come from NVMe Identify Controller (§2.1) and the descriptor is
used only for bus type, removability and its own serial spelling.

`IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` (`0x700A0`, 112 bytes returned, probe 7
byte dump):

```
67 e6 01 00 00 00 00 00   Cylinders = 124519
0c 00 00 00               MediaType = 12 (FixedMedia)
ff 00 00 00 / 3f 00 00 00 TracksPerCylinder = 255, SectorsPerTrack = 63
00 02 00 00               BytesPerSector = 512
00 60 a5 77 ee 00 00 00   <- DiskSize at offset 24 = 1024209543168
18 00 00 00               Data[0..4] = SizeOfPartitionInfo = 24
01 00 00 00               Data[4..8] = PARTITION_STYLE_GPT
```

`DiskSize` at offset 24 is `1_024_209_543_168` bytes and matches
`MSFT_PhysicalDisk.Size` exactly, so the reported capacity is the raw device
size (953.87 GiB), not the usable-volume size. `Win32_DiskDrive.Size`
reports `1_024_203_640_320` — 5,902,848 bytes smaller — and is therefore **not**
used for capacity; the two are different quantities (raw media vs. addressable
disk area) and the report names which one it prints.

### 2.1 NVMe Identify Controller

Request: `PropertyId = 49` (`StorageAdapterProtocolSpecificProperty`),
`QueryType = 0` (`PropertyStandardQuery`), then
`STORAGE_PROTOCOL_SPECIFIC_DATA` at offset 8 of the input buffer with
`ProtocolType = 3` (`ProtocolTypeNvme`), `DataType = 1` (`NVMeDataTypeIdentify`),
`ProtocolDataRequestValue = 1` (CNS = Identify Controller),
`ProtocolDataOffset = 40`, `ProtocolDataLength = 4096`.
Output: `STORAGE_PROTOCOL_DATA_DESCRIPTOR` (`Version=0x30`, `Size=0x30`,
`ProtocolDataOffset=40`, `ProtocolDataLength=4096`) followed by the 4096-byte
identify payload at descriptor offset **48** (`8 + ProtocolDataOffset`).

| identify offset | field | live value |
|---|---|---|
| 0 | VID | `0x1CC4` (Union Memory) |
| 2 | SSVID | `0x1CC4` |
| 4..24 | SN (ASCII, space-padded) | `FDBS26010EA61912B1` |
| 24..64 | MN (ASCII) | `UMIS UPJYJ1TBMNV1QWY` |
| 64..72 | FR (ASCII) | `1.0L06D1` |
| 280 | TNVMCAP | 0 (not reported by this firmware) |

**Two distinct serials are real and must not be merged:**
the identify-controller SN `FDBS26010EA61912B1` (equal to
`MSFT_PhysicalDisk.FruId`/`AdapterSerialNumber`) and the device-descriptor
serial `5CDF_B806_1090_9BA5.` (equal to `MSFT_PhysicalDisk.SerialNumber` and to
the EUI-64 `UniqueId = eui.5CDFB80610909BA5`). The report keeps them as
separate fields with separate source labels.

## 3. NVMe SMART/Health log page

Request: same property-specific shape with `DataType = 2`
(`NVMeDataTypeLogPage`) and `ProtocolDataRequestValue = 2` (SMART/Health log
page). Output descriptor 48 bytes + 512-byte log page at offset 48;
`ProtocolDataLength` echoes 512. `PropertyId = 49` (adapter) and `50`
(`StorageDeviceProtocolSpecificProperty`) both answer identically on this
SKU; the adapter form is used.

Live values (three reads, probes 3 and 6; byte offsets are log-page offsets):

| offset | field | live value | meaning |
|---|---|---|---|
| 0 | Critical Warning | 0 | no warnings asserted |
| 1..3 | Composite Temperature | 301–303 K → 28–30 °C | |
| 3 | Available Spare | 100 % | |
| 4 | Available Spare Threshold | 10 % | |
| 5 | Percentage Used | **8 %** | wear |
| 32 | Data Units Read | 110,518,068 × 1000×512 B ≈ 56.6 TB | |
| 48 | Data Units Written | 68,841,344 × 1000×512 B ≈ 35.2 TB | |
| 64 | Host Read Commands | 1,612,073,168 | |
| 80 | Host Write Commands | 1,544,914,828 | |
| 96 | Controller Busy Time | 105,504 min | |
| 112 | Power Cycles | 161 | |
| 128 | Power On Hours | **5,633** | |
| 144 | Unsafe Shutdowns | 51 | |
| 160 | Media and Data Integrity Errors | 0 | |
| 176 | Error Information Log Entries | 0 | |
| 192 | Warning Composite Temp Time | 0 min | |
| 196 | Critical Composite Temp Time | 0 min | |

Counters are stable across reads (`DataUnitsRead` identical across probes,
`PowerOnHours` 5633 in all three), so they are cached device counters, not
computed estimates.

## 4. Temperature descriptors

`PropertyId = 52` (`StorageDeviceTemperatureProperty`) and `51`
(`StorageAdapterTemperatureProperty`) both return the same 72-byte
`STORAGE_TEMPERATURE_DATA_DESCRIPTOR` (probe 4, restricted token):
`Version=40`, `Size=72`, `CriticalTemperature=87`, `WarningTemperature=84`,
`InfoCount=3`, then 16-byte `STORAGE_TEMPERATURE_INFO` entries starting at
descriptor offset 24:

| entry | Index | Temperature | OverThreshold | live |
|---|---|---|---|---|
| 0 | 0 | 28 °C | 84 | composite |
| 1 | 1 | 31 °C | −274 (`0xFEEE`, unset) | sensor 1 |
| 2 | 2 | 28 °C | −274 (unset) | sensor 2 |

The `-274` over-threshold is the NVMe "no threshold" encoding and is dropped
rather than reported as a limit. The composite reading here (28–31 °C) differs
by ~1–2 °C from the log-page composite (28–30 °C) and from
`MSFT_StorageReliabilityCounter.Temperature` (31 °C) because each is sampled at
a different instant through a different provider; the report prints one
temperature plus its source name rather than averaging them.

## 5. Health status

`PropertyId = 56` (`StorageDeviceManagementStatus`) → 84-byte
`STORAGE_DEVICE_MANAGEMENT_STATUS`: `Version=100`, `Size=84`, `Health=3`
(`DiskHealthHealthy`), `NumberOfOperationalStatus=1`,
`OperationalStatus[0]=0` (`StorageDeviceOperationalStatusUnknown` /
no additional status), `NumberOfAdditionalReasons=0`. This agrees with
`MSFT_PhysicalDisk.HealthStatus=0` (`Healthy`) and with
`Get-PhysicalDisk.HealthStatus=Healthy` observed while elevated.

Health is therefore derived from three observable, independent facts and
reported as: management-status health, SMART critical-warning word, and
media/data-integrity error count — never as a single "OK" verdict that hides
which one was silent.

## 6. Rejected sources (negative results, do not re-explore)

### 6.1 Storage-namespace and CIMv2 WMI are denied to a non-admin token

Probes 4 and 5 ran the identical calls under
`runas /trustlevel:0x20000` (`BUILTIN\Administrators` → *Group used for deny
only*, `IsInRole(Administrator)=False`):

```
[MSFT_PhysicalDisk restricted]               ERR 拒绝访问
[Get-StorageReliabilityCounter restricted]   ERR 拒绝访问
[Win32_DiskDrive cimv2 restricted]           ERR 拒绝访问
[Win32_ComputerSystem restricted]            ERR 拒绝访问
```

`MSFT_PhysicalDisk` still read fine in the ordinary (High mandatory level)
SSH session, so this is a privilege boundary, not a missing class. Consequence:
`storage status` must not depend on WMI, or it would only work when the CLI is
elevated. The IOCTL rows in §1–§5 stayed available in both contexts.

### 6.2 `MSFT_StorageReliabilityCounter` is not a wear/POH source here

While elevated (probe 2):

```
PowerOnHours = []        LoadUnloadCycleCount = []      ReadErrorsTotal = []
Wear = 0                 Temperature = 31              TemperatureMax = 84
```

The drive itself reports `Percentage Used = 8` and `Power On Hours = 5633`
(§3). Reporting `Wear = 0` would be a false statement about the drive, so the
counter's wear/power-on-hours fields are not used; only its temperature agrees
with the log page and even that is duplicated by §4.

### 6.3 Remaining dead ends

- `root\wmi MSStorageDriver_FailurePredictStatus/Data/Thresholds`: class exists,
  every instance query returns `不支持` (not supported) — this is the SATA/AHCI
  SMART bridge, and `stornvme` does not publish it for this device.
- No `root\wmi` class matching `Storage|Disk|Nvme|Smart` exists (probe 1 §I
  empty).
- NVMe Identify **Namespace** through the protocol-specific property
  (`DataType=1`, `ProtocolDataRequestValue=0`) fails with
  `ERROR_INVALID_FUNCTION (1)`; namespace capacity is therefore taken from
  `DISK_GEOMETRY_EX`, which works (§2).
- `IOCTL_DISK_GET_LENGTH_INFO` (`0x7405C`) and `IOCTL_STORAGE_READ_CAPACITY`
  (`0x2D5120`) both return `ERROR_ACCESS_DENIED` on a desired-access-0 handle,
  so neither can replace the geometry call without privilege.
- `\\.\EnergyDrv` (`AcpiVpc.sys`) exposes no drive-health verb; its complete
  documented IOCTL→ACPI dispatch table is SALS/HALS, GBMD, ITHC, GBID, SMTF,
  MHQI, KBLC/KBLR/KBLA, BTMC, BSIF, BNMC (docs/interfaces.md §11.4).
- The vendor's own storage diagnostics (`smartmontools.zip`, `storelib.dll`,
  `NVME_SMART_*` test ids) are shipped but unused by this implementation; the
  IOCTL path above is the same kernel surface they would reach.

## 7. Privilege matrix (measured, not assumed)

| call | High mandatory level (SSH here) | restricted token (`/trustlevel:0x20000`) |
|---|---|---|
| `CreateFileW(\\.\PHYSICALDRIVE0, 0, …)` | ok | ok |
| … with `GENERIC_READ` | ok | **fail, err 5** |
| device descriptor / identify / SMART / temperature / management status on the 0-access handle | ok | **ok** |
| `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` on the 0-access handle | ok | **ok** |
| `root\Microsoft\Windows\Storage` reads | ok | deny |
| `root\cimv2` reads | ok | deny |

Observed token detail worth recording: the SSH session that ran probe 3 reports
`Mandatory Label\High Mandatory Level` with `BUILTIN\Administrators` enabled and
`NT AUTHORITY\NETWORK` present, i.e. an elevated network logon — earlier notes
describing this session as non-elevated are superseded by this measurement.
The restricted-token column keeps a High integrity label but a deny-only
Administrators group, so it isolates *administrative rights* rather than
integrity level; a plain medium-integrity interactive user was not available to
test, and this document does not claim to have measured one.

## 8. Admission rules the implementation follows

1. A field is reported only when the device answered; each field carries the
   exact request that produced it, and a refusal becomes a recorded issue, not
   a zero or a guess.
2. No value is derived across sources: capacity comes from geometry, wear from
   the log page, temperature from the temperature descriptor with the log-page
   composite as a separately labelled fallback.
3. `Unavailable`/`Unsupported` per field is preferred to a synthetic default —
   in particular `Wear = 0`, `PowerOnHours = 0` and `Temperature = 0` are never
   emitted for a field the device refused.
4. Read-only means the module has no write verb at all: the only IOCTLs it
   issues are `IOCTL_STORAGE_QUERY_PROPERTY`, `IOCTL_STORAGE_GET_DEVICE_NUMBER`
   and `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX`, and the handle is opened with
   `dwDesiredAccess = 0`.

## 9. Implementation map (source state, not a new measurement)

The report above is the recovered contract; this section records how the
implementation maps onto it, so a reviewer can check the code against §2–§8
without re-deriving the probe.

| layer | file | content |
|---|---|---|
| core types | `crates/lctrl-core/src/storage.rs` | `StorageStatus`/`StorageDevice`, `StorageFieldProvenance` (one label per field naming the request that produced it), `StorageBusType` (11 = `Sata`, 17 = `Nvme`, unknown values stay `Other` beside `bus_type_raw`), `StorageMediaType`, `StorageHealth` (unknown codes stay `Unknown`), `StorageCriticalWarning`, `StorageTemperatureSensor`, `StorageSourceIssue` |
| operation | `crates/lctrl-hal/src/storage.rs` | `StorageStatusControl::storage_status` — the only verb; no self-test, sanitize, secure-erase or firmware-slot method exists |
| seam | `crates/lctrl-hal-win/src/storage.rs` | `StorageProbeApi` (`disks`/`query`/`geometry`) plus `WindowsStorage<P>`, so recorded structure bytes drive the composition in tests; `decode_*` functions decode each structure with a declared-size check |
| native | same file, `mod native` | `CM_Get_Device_Interface_List_SizeW`/`CM_Get_Device_Interface_ListW` on `GUID_DEVINTERFACE_DISK`, then `CreateFileW(…, dwDesiredAccess = 0, …)` and `DeviceIoControl` for the three IOCTLs of §8.4 |
| capability | `crates/lctrl-hal-win/src/windows_hal.rs` | records `storage.status` and `channel.storage` once per `capabilities()` call from `storage::probe_storage()`: the interface enumeration plus one `StorageDeviceProperty` request on the first disk. A per-field refusal is *not* a capability failure — it is recorded per field by the command |
| CLI | `crates/vantage-cli/src/lib.rs`, `capability.rs` | `vantage storage [status]` → `CommandId::STORAGE_STATUS`, `StateLayer::NotApplicable`, `ServiceRole::Storage` bound to `channel.storage` |

`probe_storage()` returning an empty list is a success, not a failure: the
class answered and holds no disk, which the command reports as an empty device
list plus a `GUID_DEVINTERFACE_DISK` issue (§2, §8.1).

### 9.1 Not yet observed

Everything in §2–§7 was measured on the target; the following are **not**
measured and must not be reported as verified:

- no `vantage storage` invocation has been run on 21VG, in either token;
- the human rendering and the JSON projection of `StorageStatus` have not been
  compared against the recorded values of §2–§5;
- the `capability` matrix rows for `storage.status` / `channel.storage` have
  not been read back on the target;
- the temperature-descriptor fallback path of §8.2 has no live instance on this
  SKU (the descriptor answered), so it is exercised by recorded bytes only.

Meaningful next observations: `vantage --json storage` compared field by field
with §2–§5; the same command under `/trustlevel:0x20000` to re-confirm §7; and
`vantage capability` read back for the two capability names above.

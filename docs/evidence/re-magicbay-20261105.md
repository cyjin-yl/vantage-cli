# MagicBay — accessory detect + LTE contract — 2026-11-05

Target `21VG` (ThinkBook, build 26300). All facts below are either read from a
file on the target, produced by a live read-only probe on the target, or marked
`[INFERENCE]`. No device, service, or registry value was written.

## 1. Owner binaries

| Path | Version | Role |
|------|---------|------|
| `C:\Program Files\Lenovo\LenovoMagiCenter\LenovoMagiCenterDetector.exe` | 2.0.2.1 | accessory presence watcher (this is the whole detect contract) |
| `C:\Program Files\Lenovo\LenovoMagiCenter\MagiCenter.exe` | 2.0.5.1438 | Electron host; per-accessory plugin UI |
| `C:\Program Files\Lenovo\LenovoMagiCenter\resources\plugins\` | — | contains only `_blank`; plugin payloads are fetched on demand |
| `C:\ProgramData\Lenovo\MagiCenter\update.xml` | 2.0.6.600 | updater manifest for the MagiCenter bundle (`installType 10`) |
| `HKLM\SOFTWARE\Lenovo\LenovoMagiCenter` | — | `CID = UDC` only; no detector logging flag set |

There is no Lenovo MagicBay *service* or *driver*: the accessory is a plain USB
device (`VID_17EF`) on the 21VG magnetic connector, and presence is observed by
this one user-mode detector. `LenovoMagiCenterDetector.exe` is running on the
target (PID 18920, started 2026-11-04 02:19), i.e. this is the live path.

## 2. Detect contract (recovered from IL)

`monodis` of the shipped assembly (SHA-free, version 2.0.2.1) plus a manual
decode of the one method `monodis` truncates
(`LenovoMagicDetector.DeviceWatcher::CheckDeviceStateAndUpdateCurrent`,
fat header at RVA 0x334c, 492-byte body) gives:

### 2.1 Constant table

```
BaseDefinition / DeviceWatcher (both classes carry an identical copy):
  HUDDeviceVid  = "VID_17EF"
  LTEDeviceVid  = "VID_17EF"
  TIKODeviceVid = "VID_17EF"
  HUDDevicePidLists  = { "PID_1117" -> true }
  LTEDevicePidLists  = { "PID_7005" -> true }
  TIKODevicePidLists = { "PID_62A5" -> true }
  SingleInstanceMutexName = "Global\LenovoMagiCenterDetector_SingleInstance"
  StartCenterMiniWindowPath = "SOFTWARE\Lenovo\LenovoMagiCenter", key "StartMiniWindow"
  MagiCenterUninstallRegKey = "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{753D4654-HYBR-IDDC-91EB-AF54DF8CD402}_is1"
```

`DeviceWatcher/DeviceType` is `{ Unknown = 0, HUD = 1, LTE = 2, TIKO = 3 }`
(IL constants `stind.i4 1/2/3` in `TryGetMagicType`).

### 2.2 Initial scan query (exact)

`CheckDeviceStateAndUpdateCurrent` builds a `HashSet<string>` of the three VID
constants with `StringComparer.OrdinalIgnoreCase` (so the set collapses to one
element), maps each to `vid.Trim().ToUpperInvariant().Replace("_", "[_]")`, and
adds the clause `"DeviceID LIKE '%" + escaped + "%'"`. The clauses are joined
with `" OR "` (`String.Join`) between the literal prefix
`"SELECT DeviceID FROM Win32_PnPEntity WHERE ("` and the literal suffix
`") AND DeviceID LIKE '%PID[_]%'"`. With one distinct VID the executed query is:

```sql
SELECT DeviceID FROM Win32_PnPEntity WHERE (DeviceID LIKE '%VID[_]17EF%') AND DeviceID LIKE '%PID[_]%'
```

Namespace literal `\\.\root\cimv2`; the selected column is `DeviceID`;
the query is logged as `Initial scan WMI query: …` (logging is off on this
target: `C:\ProgramData\Lenovo\MagiCenter\Logs\detector` does not exist).

Live validation on the target (read-only): the query above is accepted by WMI
and returns 0 rows today — no `VID_17EF` device is attached.

### 2.3 Live updates

`WM_DEVICECHANGE` on a message-only window (`HWND_MESSAGE`) with
`RegisterDeviceNotification(..., GUID_DEVINTERFACE_USB_DEVICE,
DEVICE_NOTIFY_WINDOW_HANDLE)`; `DBT_DEVICEARRIVAL` /
`DBT_DEVICEREMOVECOMPLETE`; the payload `DEV_BROADCAST_DEVICEINTERFACE.dbcc_name`
is parsed with the literals `VidRegex = "VID_([0-9A-Fa-f]{4})"`,
`PidRegex = "PID_([0-9A-Fa-f]{4})"`, and the extracted strings are compared
verbatim (`"VID_" + group`, `"PID_" + group`, upper-cased) against the table in
§2.1. Events are debounced with `DebounceMs = 250`.

Presence is therefore *presence only*: the detector never opens the accessory,
never talks MBIM, and never reads an accessory property. When a device matches it
launches `MagiCenter.exe` (args `--sp`, `--smw`) and otherwise stays idle.

### 2.4 Consequence for `vantage magicbay detect`

The repo table carried `0x62b5` for the Tiko accessory. That is wrong:
`0x62A5 = 25253` (the decimal in `docs/06-magicbay.md` §2.1) and the shipped
detector matches the literal `PID_62A5`. `0x62b5 = 25269` matches nothing
Lenovo ships. Fixed in `lctrl-core::KNOWN_MAGICBAY_DEVICES`; `0x62b5` now
correctly resolves to `MagicBayKind::Unknown`.

The SetupAPI path (`DIGCF_PRESENT | DIGCF_ALLCLASSES`, instance-id substring
`VID_17EF&PID_`) enumerates the same device set as the WMI query; a composite
accessory exposes both a parent and an `&MI_00` interface record, so the
existing coalescing is still required. `[INFERENCE]` the two paths agree for
present devices; `Win32_PnPEntity` may additionally list non-present phantoms,
which `DIGCF_PRESENT` intentionally excludes.

## 3. LTE contract

### 3.1 Lenovo has no LTE protocol

`docs/06-magicbay.md` §4.1 (full-text search of the MagiCenter components: zero
`MBIM`/`wwan`/`APN`/`SIM` hits) is confirmed by this detector: it has no modem
code at all. With an accessory attached the module enumerates as a **standard
MBIM** function `USB\VID_17EF&PID_7005&MI_00\…` under class `Net`, service
`cxwmbclass` (observed on this target earlier, `docs/06-magicbay.md` §2.2).
Windows therefore owns the data session; `lte2-plugin` is only a UI over the OS.

### 3.2 Runtime state of the Windows modem stack on this build

| Probe | Result |
|-------|--------|
| `Get-Service WwanSvc` | `Running`, `Automatic` |
| `Get-Service cxwmbclass` | present, `Stopped` (no MBIM function bound) |
| `netsh mbn show interfaces` | `There is no Mobile Broadband interface` |
| `C:\Windows\System32\mbnapi.dll` | **absent**; also absent from `SysWOW64` |
| Windows Kits headers | `mbnapi.h` present (10.0.22621.0, 10.0.26100.0); no `wwanapi.h` (WDK-only) |
| `C:\Windows\System32\wwanapi.dll` | 10.0.26100.9549; export directory has exactly **two** names: `DllCanUnloadNow`, `DllGetClassObject` |
| COM classes whose `InprocServer32` is `wwanapi.dll` | `{BDFEE05A-4418-11DD-90ED-001C257CCFF1}`, `{BDFEE05B-4418-11DD-90ED-001C257CCFF1}`, `{BDFEE05C-4418-11DD-90ED-001C257CCFF1}`, `{2269DAA3-2A9F-4165-A501-CE00A6F7A75B}` |
| COM class from `WwanRadioManager.dll` | `{1910E202-236A-43E6-9469-FE0B3149F3D9}` |

Consequences:

* The classic **MBN API** (`mbnapi.h`: `IMbnInterfaceManager`,
  `IMbnConnection::Connect/Disconnect`, `IMbnConnectionProfileManager`) that the
  UI layer is documented to use (`docs/06-magicbay.md` §4.3) has **no runtime
  library on this build**. It cannot be the implementation channel.
* The Windows 11 **WWAN Service API** is present but is COM-only and its
  `IWwan*` interfaces are declared only in the WDK `wwanapi.h`, which is not
  installed on this machine. `[INFERENCE]` the four `wwanapi.dll` classes are the
  WWAN connection/interface/notification/profile managers and the fifth is the
  radio manager (one DLL per class is the documented service layout); the exact
  IIDs and vtable order cannot be recovered from this machine and are **not**
  guessed here. Writing them without a header and without a module to exercise
  them would be fabricated, so `lte connect` / `lte disconnect` / `lte apn`
  stay `Unsupported`.
* No `17EF` PnP device and no WWAN net interface exist right now, so no
  session-level readback is possible either.

### 3.3 What *is* reachable read-only (implemented)

The WWAN function, when present, is an ordinary NDIS interface of type
`IF_TYPE_WWANPP = 243` / `IF_TYPE_WWANPP2 = 244` (Windows `IF_TYPE` enum), so
its link state and PDP-context addresses are published by the public CIM
providers. Live probe on this target (read-only, 8 adapters / 23 addresses):

* `root\StandardCimv2: MSFT_NetAdapter`
  `SELECT InterfaceIndex, InterfaceType, InterfaceDescription, InterfaceName, MediaConnectState, InterfaceOperationalStatus FROM MSFT_NetAdapter`
  → `InterfaceType` 6/53/71 only (Ethernet / tunnel / 802.11), i.e. no 243/244;
  `MediaConnectState` 1 = connected, 2 = disconnected, 0 = unknown;
  `InterfaceOperationalStatus` 1 = up, 2 = down, 6 = not present.
  (`OperStatus` is **not** a property of this class; `InterfaceOperationalStatus`
  is. A WQL `SELECT … OperStatus …` is rejected with `WBEM_E_INVALID_QUERY`.)
* `root\StandardCimv2: MSFT_NetIPAddress`
  `SELECT InterfaceIndex, IPAddress, AddressFamily FROM MSFT_NetIPAddress`
  → `AddressFamily` 2 = IPv4, 23 = IPv6; one row per unicast address.

That is the whole read-only LTE status surface available without a modem-control
API: accessory presence (recovered detector query), WWAN interface link state,
and the addresses that a PDP context would own. Signal strength, registration
state, carrier, IMEI, SIM state and the current APN are **not** published by any
of these classes and stay unavailable until a modem-control API can be bound.

## 4. Implemented in this slice

* `magicbay detect` — unchanged algorithm, corrected Tiko PID (§2.4).
* `magicbay lte status` — new read-only implementation
  (`lctrl-hal-win::WindowsMobileBroadband`, WMI only):
  1. accessory presence via the recovered §2.2 query (independent of, and a
     cross-check for, the SetupAPI inventory);
  2. WWAN interfaces (`InterfaceType` 243/244) with media/operational state;
  3. unicast addresses per interface, and `data_session` = any non-link-local
     IPv4/IPv6 address on a WWAN interface.
  No accessory → `ChannelUnavailable` with the reason; accessory present but no
  WWAN interface → a report with an empty `interfaces` list (that state is
  observable and worth reporting, e.g. a missing MBIM driver binding).
* Everything else under `magicbay` stays `Unsupported` with a reason that names
  the recovered contract.

## 5. Not implemented, and why

| command | blocker |
|---------|---------|
| `lte connect` / `lte disconnect` / `lte apn` | no modem-control API bindable: `mbnapi.dll` absent on build 26300; `wwanapi.dll` COM interface contract needs WDK `wwanapi.h` plus a module to exercise; also a write, which this slice does not authorise |
| `cam` | MagicBay cameras are plain UVC devices with no Lenovo identifier (`docs/06-magicbay.md` §5.1/§5.3); attributing one requires the `17EF` enclosure, which no attached hardware can demonstrate here |
| `display` | the bridge is ACPI (`ACPI\QCOM2488`, `ACPI\QCOM24B7`) and is already reported by `magicbay detect`; EDID/mode switching is a display-config surface (`docs/06-magicbay.md` §6), not an accessory control |
| `watch` | hot-plug streaming belongs to the daemon event source; the contract is the recovered §2.3 `WM_DEVICECHANGE` + `GUID_DEVINTERFACE_USB_DEVICE` registration |

## 6. Reproduction (read-only)

```powershell
# detect contract / accessory presence
Get-CimInstance -Query "SELECT DeviceID FROM Win32_PnPEntity WHERE (DeviceID LIKE '%VID[_]17EF%') AND DeviceID LIKE '%PID[_]%'"
# WWAN interface surface
Get-CimInstance -Namespace root/StandardCimv2 -Query "SELECT InterfaceIndex, InterfaceType, InterfaceDescription, InterfaceName, MediaConnectState, InterfaceOperationalStatus FROM MSFT_NetAdapter"
Get-CimInstance -Namespace root/StandardCimv2 -Query "SELECT InterfaceIndex, IPAddress, AddressFamily FROM MSFT_NetIPAddress"
# stack state
Get-Service WwanSvc, cxwmbclass | Format-Table Name,Status
netsh mbn show interfaces
Test-Path C:\Windows\System32\mbnapi.dll
```

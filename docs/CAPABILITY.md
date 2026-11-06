# Capability matrix

Reference machine: ThinkBook 14 G8+ (21VG), Windows 11, BIOS SWCN22WW.
Generated from `vantage capability --json` on 2026-11-06.

Status codes: **available** = works, **limited** = partial,
**unsupported** = firmware rejects, **unavailable** = missing dependency,
**unverified** = implemented but not smoke-tested on this machine,
**external** = provided by a separate service,
**not\_applicable** = does not apply to this hardware.

## audio

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `audio.dolby` | runtime | **available** | — |
| `audio.noise-cancel` | runtime | **available** | — |
| `audio.endpoints` | runtime | **available** | — |
| `audio.volume` | runtime | **available** | — |
| `audio.mute` | runtime | **available** | — |

## battery

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `battery.status` | runtime | **available** | — |
| `battery.adapter` | runtime | **available** | — |
| `battery.health` | runtime | **available** | — |
| `battery.charge-mode` | runtime | **available** | — |
| `battery.thresholds` | runtime | **unsupported** | custom charge thresholds have no verified native write and readback contract |
| `battery.extreme-life` | runtime | **unsupported** | extreme life is the AiTurbo run-state policy (BatteryLife.exe/AiPerfSvc), not a charge-firmware mode: no GBMD/BNMC/BTMC … |
| `battery.night-charge` | runtime | **available** | — |
| `battery.temporary-mode` | runtime | **available** | — |
| `battery.watch` | runtime | **available** | — |

## bios

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `bios.list` | bios_persistent | **available** | — |
| `bios.get` | bios_persistent | **available** | — |
| `bios.set` | bios_persistent | **available** | — |
| `bios.save` | bios_persistent | **unsupported** | standalone global BIOS-buffer save is not supported |
| `bios.discard` | bios_persistent | **available** | — |
| `bios.defaults` | bios_persistent | **available** | — |
| `bios.password.status` | bios_persistent | **available** | — |
| `bios.password.set` | bios_persistent | **unsupported** | BIOS password writes stay refused by the documented no-password-write boundary (docs/interfaces.md §0) |
| `bios.password.clear` | bios_persistent | **unsupported** | BIOS password writes stay refused by the documented no-password-write boundary (docs/interfaces.md §0) |
| `bios.password.verify` | bios_persistent | **available** | — |

## bluetooth

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `bluetooth.list` | not_applicable | **available** | — |

## completions

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `completions` | not_applicable | **available** | — |

## cpu

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `cpu.status` | not_applicable | **available** | — |

## daemon

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `daemon.start` | runtime | **external** | **missing** |
| `daemon.stop` | runtime | **external** | **missing** |
| `daemon.status` | runtime | **external** | **missing** |
| `daemon.install` | os_persistent | **external** | **missing** |

## doctor

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `doctor` | not_applicable | **available** | — |

## events

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `events.log` | runtime | **available** | — |

## firmware

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `firmware.status` | bios_persistent | **available** | — |

## fix

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `fix.rules` | not_applicable | **available** | — |
| `fix.scan` | runtime | **available** | — |
| `fix.run` | local_persistent | **available** | — |
| `fix.track` | local_persistent | **available** | — |

## hotkey

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `hotkey.list` | runtime | **available** | — |
| `hotkey.status` | composite | **available** | — |
| `hotkey.register` | os_persistent | **available** | — |
| `hotkey.trigger` | runtime | **available** | — |
| `hotkey.unregister` | os_persistent | **available** | — |

## info

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `info` | not_applicable | **available** | — |

## kbd

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `kbd.backlight` | runtime | **available** | — |
| `kbd.fnlock` | bios_persistent | **available** | — |
| `kbd.fn-ctrl-swap` | bios_persistent | **available** | — |
| `kbd.winlock` | runtime | **unsupported** | firmware rejects the Win-key methods: LENOVO_GAMEZONE_DATA.GetWinKeyStatus/SetWinKeyStatus return WBEM_E_INVALID_OBJECT … |

## magicbay

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `magicbay.detect` | not_applicable | **limited** | SetupAPI inventory is provided by the separate MagicBay service |
| `magicbay.lte.status` | runtime | **not_applicable** | **missing** |
| `magicbay.lte.connect` | runtime | **unsupported** | standard MBIM control is not attached |
| `magicbay.lte.disconnect` | runtime | **unsupported** | standard MBIM control is not attached |
| `magicbay.lte.apn` | os_persistent | **unsupported** | standard MBIM control is not attached |
| `magicbay.cam` | runtime | **unsupported** | standard UVC camera control is not attached |
| `magicbay.display` | runtime | **not_applicable** | **missing** |
| `magicbay.watch` | runtime | **unsupported** | no documented MagicBay event class exists |

## osd

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `osd.enable` | os_persistent | **available** | — |
| `osd.disable` | os_persistent | **available** | — |
| `osd.test` | runtime | **available** | — |
| `osd.show` | runtime | **available** | — |

## panel

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `panel.rate` | runtime | **available** | — |
| `panel.info` | runtime | **available** | — |
| `panel.brightness.status` | runtime | **available** | — |
| `panel.brightness.set` | runtime | **available** | — |
| `panel.color` | runtime | **unsupported** | firmware rejects the gamut plane: every LENOVO_PANEL_METHOD method including Panel_Set_Gamut_Switch and Panel_Get_Gamut_… |
| `panel.super-resolution` | runtime | **unsupported** | IGCL is the vendor channel (SESuperResolutionPlugin64.dll -> IntelControlLib.dll), but this driver answers CTL_RESULT_ER… |
| `panel.overdrive` | runtime | **unsupported** | firmware advertises no overdrive feature on this SKU: the vendor owner (SmartPanelAddin -> BiosOD.dll) drives LENOVO_OTH… |
| `panel.eye-care` | runtime | **unsupported** | vendor owner recovered (Vantage SmartInteractAddin EyeCareMode + PCManager LenovoMonitorManager.exe) and the mechanism i… |

## perf

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `perf.mode` | runtime | **limited** | Windows power overlay covers balanced, quiet and performance |
| `perf.fan.status` | runtime | **available** | — |
| `perf.fan.auto` | runtime | **unsupported** | GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode) |
| `perf.fan.manual` | runtime | **unsupported** | GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode) |
| `perf.fan.fullspeed` | runtime | **unsupported** | GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode) |
| `perf.fan.smart` | runtime | **unsupported** | GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode) |
| `perf.fan.curve` | runtime | **unsupported** | firmware rejects the fan-table plane: LENOVO_FAN_METHOD.Fan_Get_Table/Fan_Set_Table return WBEM_E_NOT_SUPPORTED on this … |
| `perf.temp` | runtime | **available** | — |
| `perf.temp.read` | runtime | **available** | — |
| `perf.temp.watch` | runtime | **available** | — |
| `perf.pl1` | runtime | **unsupported** | Windows raw MSR/RAPL writes are unavailable |
| `perf.pl2` | runtime | **unsupported** | Windows raw MSR/RAPL writes are unavailable |
| `perf.dbdc.get` | runtime | **available** | — |
| `perf.gpu.status` | runtime | **unsupported** | LENOVO_GAMEZONE_GPU_OC_DATA returned no instance |
| `perf.policy` | runtime | **available** | — |
| `perf.top` | not_applicable | **available** | — |
| `perf.boost` | not_applicable | **available** | — |
| `perf.throttle` | not_applicable | **available** | — |

## power

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `power.scheme` | os_persistent | **available** | — |
| `power.scheme.list` | os_persistent | **available** | — |
| `power.scheme.get` | os_persistent | **available** | — |
| `power.scheme.setting.read` | os_persistent | **available** | — |
| `power.scheme.apply` | os_persistent | **available** | — |
| `power.scheme.set` | os_persistent | **available** | — |
| `power.saver-once` | runtime | **available** | — |
| `power.ac` | runtime | **available** | — |
| `power.draw` | runtime | **available** | — |

## privacy

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `privacy.cam` | runtime | **available** | — |
| `privacy.cam` | bios_persistent | **available** | — |
| `privacy.mic` | runtime | **available** | — |
| `privacy.mic` | bios_persistent | **available** | — |
| `privacy.fingerprint` | bios_persistent | **available** | — |
| `privacy.status` | composite | **available** | — |

## radio

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `radio.status` | runtime | **available** | — |
| `radio.off` | runtime | **unverified** | radio.off has not been probed on this platform |
| `radio.on` | runtime | **unverified** | radio.on has not been probed on this platform |

## scan

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `scan.list` | not_applicable | **available** | — |
| `scan.run` | not_applicable | **available** | — |

## scene

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `scene.list` | not_applicable | **available** | — |
| `scene.test` | runtime | **available** | — |
| `scene.track` | local_persistent | **available** | — |

## security

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `security.status` | not_applicable | **available** | — |

## sense

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `sense.status` | runtime | **available** | — |
| `sense.lock-on-leave` | runtime | **available** | — |
| `sense.wake-on-approach` | runtime | **available** | — |
| `sense.pause-video` | runtime | **available** | — |
| `sense.camera-assist` | runtime | **available** | — |
| `sense.approach-auto-adjust` | runtime | **available** | — |
| `sense.leave-auto-adjust` | runtime | **available** | — |
| `sense.reset` | runtime | **available** | — |
| `sense.attention-tracking` | runtime | **available** | — |
| `sense.kbd-light-auto` | runtime | **unsupported** | automatic keyboard backlight has no consuming service on this model: ThinkSmartSenseAddin 1.0.0.77 targets LPlatSvc/LITS… |

## sensor

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `sensor.list` | not_applicable | **available** | — |
| `sensor.read` | not_applicable | **available** | — |

## snapshot

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `snapshot.capture` | local_persistent | **limited** | Windows power overlay covers balanced, quiet and performance |
| `snapshot.diff` | composite | **limited** | Windows power overlay covers balanced, quiet and performance |
| `snapshot.restore` | composite | **limited** | Windows power overlay covers balanced, quiet and performance |

## storage

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `storage.status` | not_applicable | **available** | — |

## touchpad

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `touchpad` | platform_dependent | **available** | — |
| `touchpad.status` | runtime | **available** | — |
| `touchpad.recover` | runtime | **available** | — |
| `touchpad.caps` | runtime | **available** | — |

## tune

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `tune.profile.list` | local_persistent | **available** | — |
| `tune.profile.show` | local_persistent | **available** | — |
| `tune.profile.apply` | composite | **limited** | Windows power overlay covers balanced, quiet and performance |
| `tune.pl1` | runtime | **unavailable** | Windows raw MSR/RAPL writes are unavailable |
| `tune.epp` | platform_dependent | **available** | — |
| `tune.turbo` | platform_dependent | **available** | — |
| `tune.restore` | composite | **limited** | Windows power overlay covers balanced, quiet and performance |
| `tune.telemetry` | runtime | **available** | — |
| `tune.watch` | runtime | **available** | — |

## update

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `update.check` | not_applicable | **available** | — |
| `update.local.install` | not_applicable | **available** | — |
| `update.local.package` | not_applicable | **unverified** | update.local_package has not been probed on this platform |
| `update.local.rollback` | not_applicable | **available** | — |
| `update.local.history` | local_persistent | **available** | — |
| `update.local.ignore` | local_persistent | **available** | — |
| `update.local.schedule` | local_persistent | **available** | — |
| `update.download` | local_persistent | **unsupported** | no authenticated remote update manifest is attached |
| `update.install` | not_applicable | **unsupported** | no authenticated remote update manifest is attached |
| `update.history` | local_persistent | **unverified** | update.history.ledger has not been probed on this platform |
| `update.ignore` | local_persistent | **unsupported** | no remote update catalog is attached |
| `update.rollback` | not_applicable | **unsupported** | no remote update install ledger is attached |
| `update.schedule` | local_persistent | **unsupported** | no remote update catalog is attached |

## usb

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `usb.always-on` | runtime | **available** | — |
| `usb.always-on` | bios_persistent | **available** | — |
| `usb.charge-on-battery` | runtime | **available** | — |
| `usb.charge-on-battery` | bios_persistent | **available** | — |
| `usb.ports` | runtime | **available** | — |

## wifi

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `wifi.security` | not_applicable | **available** | — |
| `wifi.status` | not_applicable | **available** | — |

## Summary

| Status | Count |
|--------|-------|
| available | 112 |
| limited | 7 |
| unsupported | 29 |
| unavailable | 1 |
| unverified | 4 |
| external | 4 |
| not_applicable | 2 |
| **total** | **159** |

## Implementation channels

### Windows

| Channel | API | Used by |
|---------|-----|---------|
| WMI | `root\WMI`, `root\CIMV2` via COM/IWbem | battery, BIOS, thermal, fan, panel, security, magicbay |
| IOCTL | `DeviceIoControl` on `\\.\EnergyDrv` | charge mode, night charge, kbd backlight, battery advanced |
| HPD RPC | Named-pipe RPC to LenovoFnAndFunctionKeys | SmartSense/HPD controls (9 commands) |
| Dolby RPC | `rpcrt4` ALPC to DolbyDAX2API | Dolby profile on/off |
| Elevoc RPC | `rpcrt4` ALPC to ElevocSDK | Noise cancellation mode |
| Win32 Power | `powrprof.dll` PowerRead/Write/SetActive | power schemes, EPP, boost, throttle |
| Registry | `advapi32` RegOpenKeyEx/RegQueryValueEx | update catalog, OSD, ITS policy, performance state |
| SetupAPI | `setupapi.dll` SetupDiGetClassDevs | USB ports, touchpad, radio, device enumeration |
| HID | `hid.dll` HidD_GetPreparsedData | touchpad caps, precision digitizer |
| Sensor API | `sensorsapi.h` ISensorManager/ISensor COM | ALS, proximity, gesture, HPD sensors |
| CCD/DPI | `user32` DisplayConfigGetDeviceInfo | display status, scaling, HDR, advanced colour |
| WinRT Radio | `Windows.Devices.Radios` | radio on/off/status |
| WLAN API | `wlanapi.dll` WlanOpenHandle | WiFi link state, security, PHY info |
| Bluetooth | `bluetoothapis.h` BluetoothFind* | device enumeration, radio state |
| MMDevice | `mmdevapi` IMMDeviceEnumerator COM | audio endpoints, volume, mute |
| Gamma ramp | `gdi32` Get/SetDeviceGammaRamp | eye-care colour temperature |
| Event log | `wevtapi` EvtQuery/EvtNext | Windows event log reader |
| LDE SDK | `LdeApi.dll` JSON API | hardware scan/diagnostics |
| winsqlite3 | `winsqlite3.dll` (system) | update history DB |
| SMBIOS | `GetSystemFirmwareTable` | firmware inventory |

### Linux

| Channel | Path/API | Used by |
|---------|----------|---------|
| sysfs thermal | `/sys/class/thermal/thermal_zone*` | temperature read |
| sysfs hwmon | `/sys/class/hwmon/hwmon*/fan*_input` | fan RPM |
| sysfs ideapad | `/sys/bus/platform/drivers/ideapad_acpi` | performance mode, fan mode, conservation |
| sysfs backlight | `/sys/class/backlight/*/brightness` | panel brightness |
| sysfs leds | `/sys/class/leds/*/brightness` | keyboard backlight |
| sysfs power | `/sys/class/power_supply/BAT*` | battery status, health |
| procfs | `/proc/cpuinfo`, `/proc/stat` | CPU info, process sampling |
| DRM | `/sys/class/drm/card*/` | display mode, refresh rate |
| powercap | `/sys/class/powercap/intel-rapl*` | PL1/PL2/tau (when available) |
| ACPI | `/sys/firmware/acpi/tables` | SMBIOS, BIOS settings |

## Unsupported features — detailed reasons

Each unsupported entry has a specific reason tied to firmware probing
on the reference machine.  These are not TODO items; they reflect real
hardware limits.  A different machine model may support some of these.

### `battery.extreme-life`

extreme life is the AiTurbo run-state policy (BatteryLife.exe/AiPerfSvc), not a charge-firmware mode: no GBMD/BNMC/BTMC selector carries it, and the PCManager charge radio's extra state is night charge (BNMC 0x83102150), not a fourth firmware mode. Full equivalence needs an owned signed kernel Job lifecycle implementation and the target UI toggle mapping; a partial power-plan or EcoQoS substitute is not implemented (docs/evidence/extreme-life-re-20261102.md, docs/evidence/re-battery-adv-20261105.md)

### `battery.thresholds`

custom charge thresholds have no verified native write and readback contract; fixed conservation mode is not a custom threshold interface (docs/02-power-battery.md §4.2)

### `bios.password.clear`

BIOS password writes stay refused by the documented no-password-write boundary (docs/interfaces.md §0)

### `bios.password.set`

BIOS password writes stay refused by the documented no-password-write boundary (docs/interfaces.md §0)

### `bios.save`

standalone global BIOS-buffer save is not supported

### `kbd.winlock`

firmware rejects the Win-key methods: LENOVO_GAMEZONE_DATA.GetWinKeyStatus/SetWinKeyStatus return WBEM_E_INVALID_OBJECT on this SKU (docs/evidence/wmi-capability-exhaustion-20261104.md §4)

### `magicbay.cam`

standard UVC camera control is not attached

### `magicbay.lte.apn`

standard MBIM control is not attached

### `magicbay.lte.connect`

standard MBIM control is not attached

### `magicbay.lte.disconnect`

standard MBIM control is not attached

### `magicbay.watch`

no documented MagicBay event class exists

### `panel.color`

firmware rejects the gamut plane: every LENOVO_PANEL_METHOD method including Panel_Set_Gamut_Switch and Panel_Get_Gamut_Switch returns WBEM_E_NOT_SUPPORTED on this SKU, so no user-mode write path exists (docs/evidence/wmi-capability-exhaustion-20261104.md §2)

### `panel.eye-care`

vendor owner recovered (Vantage SmartInteractAddin EyeCareMode + PCManager LenovoMonitorManager.exe) and the mechanism is the system gamma ramp (GetDeviceGammaRamp/SetDeviceGammaRamp), implemented here through lctrl-hal-win's NativeGammaRamp with readback classification; the row stays unsupported because this SSH/network logon cannot exercise a desktop gamma write and the Mid/High Kelvin table is repo-inferred rather than vendor-extracted (docs/evidence/re-panel-20261105.md §5)

### `panel.overdrive`

firmware advertises no overdrive feature on this SKU: the vendor owner (SmartPanelAddin -> BiosOD.dll) drives LENOVO_OTHER_METHOD Get/SetFeatureValue with IDs 0x1A0000 but only after LENOVO_CAPABILITY_DATA_00 lists an 0x1A family, and this machine lists none (dump in docs/evidence/re-panel-20261105.md §3); the Legion plane (LENOVO_PANEL_METHOD MPRT, LENOVO_GAMEZONE_DATA OD) rejects with 不支持/无效的对象 and the TCON plane needs \\.\SPBTESTTOOL, which is absent

### `panel.super-resolution`

IGCL is the vendor channel (SESuperResolutionPlugin64.dll -> IntelControlLib.dll), but this driver answers CTL_RESULT_ERROR_INVALID_ARGUMENT (0x4000000b) to both the two-pass capability enumeration and the get/set state query, so there is no previous state to verify or roll back; the write is guarded by that read and stays closed (docs/evidence/re-panel-20261105.md §4)

### `perf.fan.auto`

GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode); SetSmartFanMode acknowledges calls without validating the value and its result cannot be read back, so a fan-mode write would be unverifiable (docs/evidence/re-fan-write-20261105.md)

### `perf.fan.curve`

firmware rejects the fan-table plane: LENOVO_FAN_METHOD.Fan_Get_Table/Fan_Set_Table return WBEM_E_NOT_SUPPORTED on this SKU (docs/evidence/wmi-capability-exhaustion-20261104.md §3)

### `perf.fan.fullspeed`

GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode); SetSmartFanMode acknowledges calls without validating the value and its result cannot be read back, so a fan-mode write would be unverifiable (docs/evidence/re-fan-write-20261105.md)

### `perf.fan.manual`

GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode); SetSmartFanMode acknowledges calls without validating the value and its result cannot be read back, so a fan-mode write would be unverifiable (docs/evidence/re-fan-write-20261105.md)

### `perf.fan.smart`

GetSmartFanMode unavailable (feature is unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode); SetSmartFanMode acknowledges calls without validating the value and its result cannot be read back, so a fan-mode write would be unverifiable (docs/evidence/re-fan-write-20261105.md)

### `perf.gpu.status`

LENOVO_GAMEZONE_GPU_OC_DATA returned no instance

### `perf.pl1`

Windows raw MSR/RAPL writes are unavailable

### `perf.pl2`

Windows raw MSR/RAPL writes are unavailable

### `sense.kbd-light-auto`

automatic keyboard backlight has no consuming service on this model: ThinkSmartSenseAddin 1.0.0.77 targets LPlatSvc/LITSSvc registry parameters, and both services are absent on 21VG (docs/interfaces.md §13.3)

### `tune.pl1`

Windows raw MSR/RAPL writes are unavailable

### `update.download`

no authenticated remote update manifest is attached

### `update.ignore`

no remote update catalog is attached

### `update.install`

no authenticated remote update manifest is attached

### `update.rollback`

no remote update install ledger is attached

### `update.schedule`

no remote update catalog is attached

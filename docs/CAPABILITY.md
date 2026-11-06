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
| `audio.dolby` | runtime | **available** |  |
| `audio.noise-cancel` | runtime | **available** |  |
| `audio.endpoints` | runtime | **available** |  |
| `audio.volume` | runtime | **available** |  |
| `audio.mute` | runtime | **available** |  |

## battery

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `battery.status` | runtime | **available** |  |
| `battery.adapter` | runtime | **available** |  |
| `battery.health` | runtime | **available** |  |
| `battery.charge-mode` | runtime | **available** |  |
| `battery.thresholds` | runtime | **unsupported** |  |
| `battery.extreme-life` | runtime | **unsupported** |  |
| `battery.night-charge` | runtime | **available** |  |
| `battery.temporary-mode` | runtime | **available** |  |
| `battery.watch` | runtime | **available** |  |

## bios

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `bios.list` | bios_persistent | **available** |  |
| `bios.get` | bios_persistent | **available** |  |
| `bios.set` | bios_persistent | **available** |  |
| `bios.save` | bios_persistent | **unsupported** |  |
| `bios.discard` | bios_persistent | **available** |  |
| `bios.defaults` | bios_persistent | **available** |  |
| `bios.password.status` | bios_persistent | **available** |  |
| `bios.password.set` | bios_persistent | **unsupported** |  |
| `bios.password.clear` | bios_persistent | **unsupported** |  |
| `bios.password.verify` | bios_persistent | **available** |  |

## bluetooth

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `bluetooth.list` | not_applicable | **available** |  |

## completions

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `completions` | not_applicable | **available** |  |

## cpu

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `cpu.status` | not_applicable | **available** |  |

## daemon

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `daemon.start` | runtime | **external** |  |
| `daemon.stop` | runtime | **external** |  |
| `daemon.status` | runtime | **external** |  |
| `daemon.install` | os_persistent | **external** |  |

## doctor

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `doctor` | not_applicable | **available** |  |

## events

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `events.log` | runtime | **available** |  |

## firmware

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `firmware.status` | bios_persistent | **available** |  |

## fix

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `fix.rules` | not_applicable | **available** |  |
| `fix.scan` | runtime | **available** |  |
| `fix.run` | local_persistent | **available** |  |
| `fix.track` | local_persistent | **available** |  |

## hotkey

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `hotkey.list` | runtime | **available** |  |
| `hotkey.status` | composite | **available** |  |
| `hotkey.register` | os_persistent | **available** |  |
| `hotkey.trigger` | runtime | **available** |  |
| `hotkey.unregister` | os_persistent | **available** |  |

## info

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `info` | not_applicable | **available** |  |

## kbd

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `kbd.backlight` | runtime | **available** |  |
| `kbd.fnlock` | bios_persistent | **available** |  |
| `kbd.fn-ctrl-swap` | bios_persistent | **available** |  |
| `kbd.winlock` | runtime | **unsupported** |  |

## magicbay

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `magicbay.detect` | not_applicable | **limited** |  |
| `magicbay.lte.status` | runtime | **not_applicable** |  |
| `magicbay.lte.connect` | runtime | **unsupported** |  |
| `magicbay.lte.disconnect` | runtime | **unsupported** |  |
| `magicbay.lte.apn` | os_persistent | **unsupported** |  |
| `magicbay.cam` | runtime | **unsupported** |  |
| `magicbay.display` | runtime | **not_applicable** |  |
| `magicbay.watch` | runtime | **unsupported** |  |

## osd

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `osd.enable` | os_persistent | **available** |  |
| `osd.disable` | os_persistent | **available** |  |
| `osd.test` | runtime | **available** |  |
| `osd.show` | runtime | **available** |  |

## panel

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `panel.rate` | runtime | **available** |  |
| `panel.info` | runtime | **available** |  |
| `panel.brightness.status` | runtime | **available** |  |
| `panel.brightness.set` | runtime | **available** |  |
| `panel.color` | runtime | **unsupported** |  |
| `panel.super-resolution` | runtime | **unsupported** |  |
| `panel.overdrive` | runtime | **unsupported** |  |
| `panel.eye-care` | runtime | **unsupported** |  |

## perf

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `perf.mode` | runtime | **limited** |  |
| `perf.fan.status` | runtime | **available** |  |
| `perf.fan.auto` | runtime | **unsupported** |  |
| `perf.fan.manual` | runtime | **unsupported** |  |
| `perf.fan.fullspeed` | runtime | **unsupported** |  |
| `perf.fan.smart` | runtime | **unsupported** |  |
| `perf.fan.curve` | runtime | **unsupported** |  |
| `perf.temp` | runtime | **available** |  |
| `perf.temp.read` | runtime | **available** |  |
| `perf.temp.watch` | runtime | **available** |  |
| `perf.pl1` | runtime | **unsupported** |  |
| `perf.pl2` | runtime | **unsupported** |  |
| `perf.dbdc.get` | runtime | **available** |  |
| `perf.gpu.status` | runtime | **unsupported** |  |
| `perf.policy` | runtime | **available** |  |
| `perf.top` | not_applicable | **available** |  |
| `perf.boost` | not_applicable | **available** |  |
| `perf.throttle` | not_applicable | **available** |  |

## power

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `power.scheme` | os_persistent | **available** |  |
| `power.scheme.list` | os_persistent | **available** |  |
| `power.scheme.get` | os_persistent | **available** |  |
| `power.scheme.setting.read` | os_persistent | **available** |  |
| `power.scheme.apply` | os_persistent | **available** |  |
| `power.scheme.set` | os_persistent | **available** |  |
| `power.saver-once` | runtime | **available** |  |
| `power.ac` | runtime | **available** |  |
| `power.draw` | runtime | **available** |  |

## privacy

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `privacy.cam` | runtime | **available** |  |
| `privacy.cam` | bios_persistent | **available** |  |
| `privacy.mic` | runtime | **available** |  |
| `privacy.mic` | bios_persistent | **available** |  |
| `privacy.fingerprint` | bios_persistent | **available** |  |
| `privacy.status` | composite | **available** |  |

## radio

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `radio.status` | runtime | **available** |  |
| `radio.off` | runtime | **unverified** |  |
| `radio.on` | runtime | **unverified** |  |

## scan

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `scan.list` | not_applicable | **available** |  |
| `scan.run` | not_applicable | **available** |  |

## scene

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `scene.list` | not_applicable | **available** |  |
| `scene.test` | runtime | **available** |  |
| `scene.track` | local_persistent | **available** |  |

## security

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `security.status` | not_applicable | **available** |  |

## sense

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `sense.status` | runtime | **available** |  |
| `sense.lock-on-leave` | runtime | **available** |  |
| `sense.wake-on-approach` | runtime | **available** |  |
| `sense.pause-video` | runtime | **available** |  |
| `sense.camera-assist` | runtime | **available** |  |
| `sense.approach-auto-adjust` | runtime | **available** |  |
| `sense.leave-auto-adjust` | runtime | **available** |  |
| `sense.reset` | runtime | **available** |  |
| `sense.attention-tracking` | runtime | **available** |  |
| `sense.kbd-light-auto` | runtime | **unsupported** |  |

## sensor

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `sensor.list` | not_applicable | **available** |  |
| `sensor.read` | not_applicable | **available** |  |

## snapshot

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `snapshot.capture` | local_persistent | **limited** |  |
| `snapshot.diff` | composite | **limited** |  |
| `snapshot.restore` | composite | **limited** |  |

## storage

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `storage.status` | not_applicable | **available** |  |

## touchpad

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `touchpad` | platform_dependent | **available** |  |
| `touchpad.status` | runtime | **available** |  |
| `touchpad.recover` | runtime | **available** |  |
| `touchpad.caps` | runtime | **available** |  |

## tune

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `tune.profile.list` | local_persistent | **available** |  |
| `tune.profile.show` | local_persistent | **available** |  |
| `tune.profile.apply` | composite | **limited** |  |
| `tune.pl1` | runtime | **unavailable** |  |
| `tune.epp` | platform_dependent | **available** |  |
| `tune.turbo` | platform_dependent | **available** |  |
| `tune.restore` | composite | **limited** |  |
| `tune.telemetry` | runtime | **available** |  |
| `tune.watch` | runtime | **available** |  |

## update

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `update.check` | not_applicable | **available** |  |
| `update.local.install` | not_applicable | **available** |  |
| `update.local.package` | not_applicable | **unverified** |  |
| `update.local.rollback` | not_applicable | **available** |  |
| `update.local.history` | local_persistent | **available** |  |
| `update.local.ignore` | local_persistent | **available** |  |
| `update.local.schedule` | local_persistent | **available** |  |
| `update.download` | local_persistent | **unsupported** |  |
| `update.install` | not_applicable | **unsupported** |  |
| `update.history` | local_persistent | **unverified** |  |
| `update.ignore` | local_persistent | **unsupported** |  |
| `update.rollback` | not_applicable | **unsupported** |  |
| `update.schedule` | local_persistent | **unsupported** |  |

## usb

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `usb.always-on` | runtime | **available** |  |
| `usb.always-on` | bios_persistent | **available** |  |
| `usb.charge-on-battery` | runtime | **available** |  |
| `usb.charge-on-battery` | bios_persistent | **available** |  |
| `usb.ports` | runtime | **available** |  |

## wifi

| Command | Layer | Status | Reason |
|---------|-------|--------|--------|
| `wifi.security` | not_applicable | **available** |  |
| `wifi.status` | not_applicable | **available** |  |

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

no reason recorded

### `battery.thresholds`

no reason recorded

### `bios.password.clear`

no reason recorded

### `bios.password.set`

no reason recorded

### `bios.save`

no reason recorded

### `kbd.winlock`

no reason recorded

### `magicbay.cam`

no reason recorded

### `magicbay.lte.apn`

no reason recorded

### `magicbay.lte.connect`

no reason recorded

### `magicbay.lte.disconnect`

no reason recorded

### `magicbay.watch`

no reason recorded

### `panel.color`

no reason recorded

### `panel.eye-care`

no reason recorded

### `panel.overdrive`

no reason recorded

### `panel.super-resolution`

no reason recorded

### `perf.fan.auto`

no reason recorded

### `perf.fan.curve`

no reason recorded

### `perf.fan.fullspeed`

no reason recorded

### `perf.fan.manual`

no reason recorded

### `perf.fan.smart`

no reason recorded

### `perf.gpu.status`

no reason recorded

### `perf.pl1`

no reason recorded

### `perf.pl2`

no reason recorded

### `sense.kbd-light-auto`

no reason recorded

### `tune.pl1`

no reason recorded

### `update.download`

no reason recorded

### `update.ignore`

no reason recorded

### `update.install`

no reason recorded

### `update.rollback`

no reason recorded

### `update.schedule`

no reason recorded

# Implementation notes

How each feature is implemented on Windows and Linux.  Read
[CAPABILITY.md](CAPABILITY.md) for the per-command support matrix.

## Battery

### Windows

Three independent data sources, merged by the CLI:

1. **EnergyDrv IOCTL** (`\\.\EnergyDrv`).  Charge mode read/write via
   GBMD/SBMC control codes.  Night charge via BNMC `0x83102150`.
   Temporary charge mode via `0x83102141` (read-only).  Requires the
   Lenovo EnergyDrv kernel driver.

2. **OS battery class** (`IOCTL_BATTERY_QUERY_INFORMATION`,
   `IOCTL_BATTERY_STATUS`).  Design/full capacity, voltage, rate,
   cycle count, chemistry.  Works on any Windows machine with a
   battery.

3. **System power status** (`GetSystemPowerStatus`).  AC line, battery
   percent, estimated runtime.  Aggregate only — does not identify
   individual batteries.

Charge mode values: 0 = normal, 1 = conservation, 2 = rapid.
Arbitrary percentage thresholds are not supported (Storage80
capability = 0 on 21VG).

### Linux

`/sys/class/power_supply/BAT*` for status, capacity, voltage, cycle
count.  `/sys/bus/platform/drivers/ideapad_acpi/VPC*/conservation_mode`
for conservation mode toggle.

## Performance mode

### Windows

`powrprof.dll` `PowerSetActiveOverlayScheme` sets the Windows overlay
(balanced, quiet, performance).  The Lenovo ITS dispatcher
(`LenovoProcessManagement`) owns `ITS_CurrentSetting` in the
PowerSlider registry key; when `ITS_AutomaticModeSetting` is active
(value 7 on 21VG), direct writes are overridden.  The CLI writes only
the OS overlay and reports the ITS projection as read-only metadata.

Vendor-only modes (geek, silent-high-performance, custom) have no OS
overlay equivalent and are refused.

### Linux

`/sys/bus/platform/drivers/ideapad_acpi/VPC*/thermal_mode` or
`/sys/firmware/acpi/platform_profile`.  Values: 0 = balanced,
1 = performance, 2 = quiet.  Direct sysfs write with poll readback.

## Fan

### Windows

Fan mode is read from `HKLM\SYSTEM\CurrentControlSet\Services\
LenovoProcessManagement\Performance\PowerSlider\ITS_FanMode`.  On 21VG
the ITS dispatcher owns fan control exclusively:
`LENOVO_GAMEZONE_DATA.GetSmartFanMode` returns `WBEM_E_NOT_SUPPORTED`
and `SetSmartFanMode` accepts calls but has no verifiable readback.
Fan writes are refused; fan status is read-only.

Fan RPM is not readable on 21VG (no tachometer WMI class, no ACPI
`FAN` device, no hwmon fan input).

### Linux

`/sys/class/hwmon/hwmon*/fan*_input` for RPM.
`/sys/bus/platform/drivers/ideapad_acpi/VPC*/fan_mode` for fan mode
write with readback and rollback.

## Temperature

### Windows

Two sources: ACPI thermal zones (`root\WMI:MSAcpi_ThermalZoneTemperature`)
and Intel ESIF participants (`root\WMI:WmiEsif*`).  NVMe temperature
via `IOCTL_STORAGE_QUERY_PROPERTY` `StorageDeviceTemperatureProperty`.

### Linux

`/sys/class/thermal/thermal_zone*/temp` (millidegrees Celsius).
`/sys/class/hwmon/hwmon*/temp*_input` for per-sensor readings.

## Keyboard backlight

### Windows

ACPI VPC2004 KBLC method via EnergyDrv IOCTL `0x83102144`.  The VPC
keyboard group 3 exposes states 0–3: off, low, high, auto.  The write
reads back via the same IOCTL and rolls back on mismatch.

### Linux

`/sys/class/leds/platform::kbd_backlight/brightness` and
`max_brightness`.  Direct sysfs write.

## Touchpad

### Windows

`CycleInputLeaves`: enumerates HID digitizer input collections via
SetupAPI, identifies the precision touchpad by usage page 0x0D /
usage 0x0005, and toggles enabled/disabled by calling
`HidD_SetFeature` on input-leaf device nodes rather than disabling
the ACPI controller (which would lose the HID children).

Capabilities from the HID preparsed data: contact count maximum,
button type, scan time, pressure support, confidence bit.

Recovery re-enumerates and re-enables leaves that report disabled.

### Linux

`/sys/devices/*/input/input*/inhibited` for enable/disable (kernel
5.13+).

## SmartSense / HPD

### Windows

Named-pipe RPC to `LenovoFnAndFunctionKeys` service.  The HPD
protocol carries 28 command codes; 9 are implemented:

- Status (version, feature bitmask)
- Lock-on-leave, wake-on-approach, pause-video, camera-assist
- Approach/leave auto-adjust
- Attention tracking, reset

The service owns presence detection hardware (AMS TMF882x ToF
sensor array) and responds to `ChangeSetting`/`GetSetting` RPCs.

### Linux

Not available — HPD hardware is driven by a Windows-only service.

## Audio (Dolby / noise cancellation)

### Windows

Pure-Rust `rpcrt4.dll` RPC clients, not vendor DLLs:

- **Dolby**: ALPC to DolbyDAX2API endpoint (opcodes 6/8).
  Profile set/get.  The RPC interface was reverse-engineered from
  the public NDR/DCE encoding rules, not copied from vendor binaries.

- **Elevoc**: ALPC to ElevocControlSDK endpoint.  Noise cancellation
  mode set/get.  Same pure-Rust RPC transport.

Readback is `Unverified` — the RPC returns success but no independent
hardware measurement confirms the DSP state.

### Linux

Not available — Dolby and Elevoc are Windows-only audio pipelines.

## BIOS settings

### Windows

`Lenovo_BiosSetting` WMI class (`root\WMI`) for enumeration and read.
`Lenovo_SetBiosSetting` / `Lenovo_SaveBiosSettings` for staging and
commit.  The staging buffer write has verified readback.  Password
verification via `Lenovo_BiosPasswordSettings`.

Password set/clear is intentionally refused (no-password-write
boundary).

### Linux

`/sys/class/firmware-attributes/*/authentication/*/` (kernel 5.12+
thinklmi driver).

## Panel / display

### Windows

- **Refresh rate**: `ChangeDisplaySettingsExW` with validated
  `DEVMODE.dmDisplayFrequency`.  Verified 60 ↔ 120 Hz transitions
  on 21VG.
- **EDID**: per-monitor registry value under PnP device instance.
- **Display status**: CCD paths via `DisplayConfigGetDeviceInfo`,
  DPI via `GetDpiForMonitor`, advanced colour via
  `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO`.
- **Monitor colour store**: display-driver registry under
  `ControlSet\Control\GraphicsDrivers\MonitorDataStore`.
- **Eye-care**: system gamma ramp (`Get/SetDeviceGammaRamp`).
  Code exists but requires an interactive desktop session (SSH
  logon cannot exercise it).

Panel colour gamut, overdrive and super-resolution are firmware-
rejected on 21VG (see CAPABILITY.md for per-feature reasons).

### Linux

DRM mode setting via `/sys/class/drm/card*/modes`,
`/sys/class/backlight/*/brightness`.

## Security

### Windows

- **Secure Boot**: `GetFirmwareEnvironmentVariableExW` for the UEFI
  SecureBoot variable, plus registry mirror check.
- **TPM**: `root\CIMV2\Security\MicrosoftTpm:Win32_Tpm` WMI class.
- **BitLocker**: `root\CIMV2\Security\MicrosoftVolumeEncryption:
  Win32_EncryptableVolume`.

Read-only.  No writes to security configuration.

### Linux

`/sys/class/tpm/tpm*/` for TPM presence.
`/sys/firmware/efi/efivars/SecureBoot-*` for Secure Boot state.

## Storage

### Windows

`IOCTL_STORAGE_QUERY_PROPERTY` for device descriptor, SMART/health,
temperature.  NVMe: Identify Controller (CNS 0x01) for serial/firmware,
SMART/Health log page (0x02) for percentage used, spare, power-on
hours, unsafe shutdowns, media errors.

### Linux

`/sys/block/*/device/` for model, serial, firmware.
`/sys/class/nvme/*/` for NVMe health attributes.
`smartctl` integration when available.

## Update catalog

### Windows

Reads the UDC (Unified Device Configurator) synced catalog from the
registry at `HKLM\SOFTWARE\WOW6432Node\Lenovo\UDC\Packages`.  The
`LastUpdateConfig` JSON value contains package metadata; the sibling
`UpdateCheckDateTime` value provides the sync timestamp.

Remote download and install are transport-gated behind the UDC mTLS
device credential (managed by `LenovoPCMKeyService`, JWT-based,
hardware-bound).  This credential is not extractable to a foreign
client.

Local install uses Lenovo-signed `install.ps1` scripts from
already-downloaded packages.  Firmware packages are refused.

### Linux

Not applicable — UDC is a Windows-only service.

## Sensors

### Windows

Windows Sensor API (`ISensorManager` / `ISensor` COM interfaces).
Each call opens its own COM apartment, enumerates sensors, and reads
the latest data report.  No report interval or event sink is written.

On 21VG: AMS TMF882x ToF (distance), CM32183 ALS (ambient light),
AMS human detect, AMS HPD gesture, dynamic keyboard lighting,
keyboard lighting mode, AMS HOD.

### Linux

`/sys/bus/iio/devices/iio:device*/` for IIO sensor data.

## Radio

### Windows

`Windows.Devices.Radios` WinRT API for enumeration and on/off control.
`wlanapi.dll` for Wi-Fi link state, security, PHY details.  An
anti-lockout guard prevents disabling all radios simultaneously.

### Linux

`rfkill` interface via `/sys/class/rfkill/*/`.

## USB ports

### Windows

SetupAPI hub enumeration (`GUID_DEVINTERFACE_USB_HUB`) followed by
per-hub `IOCTL_USB_GET_NODE_INFORMATION` and per-port
`IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX`.  USB-C PD status
from UCSI (`ACPI\USBC000`).

### Linux

`/sys/bus/usb/devices/*/` for device tree.

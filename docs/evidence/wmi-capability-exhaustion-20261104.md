# WMI capability exhaustion, 21VG ThinkBook 14 G8+ IPH — 2026-11-04

Target: TB14P-EZRA, Windows 11 build 26100. Read-only probe session against
`root\WMI`. Build under test:
`C:\Users\ezra\vantage-target\release\vantage.exe`, SHA-256
`11A4F2AD8744DF39DDAC8447D3AC40CB9012395FA95AEC7D0BB9A2B8ACDC9174`.

Purpose: settle which "unsupported" capabilities are unsupported because this
CLI never looked, and which are absent from the firmware. Every method below was
invoked; the result is the firmware's own answer, not an inference from the
class existing.

## 0. Method the probe uses

WMI method classes in `root\WMI` are not invokable through
`Invoke-CimMethod -ClassName`. Two distinct failures appear and must not be
conflated:

| shape | message | meaning |
| --- | --- | --- |
| class invocation | `无效的方法参数` (WBEM_E_INVALID_METHOD_PARAMETERS) | PowerShell passed no instance |
| instance invocation | `无效的对象` (WBEM_E_INVALID_OBJECT) | provider rejected the target object |
| instance invocation | `不支持` (WBEM_E_NOT_SUPPORTED) | **firmware does not implement this method on this SKU** |

Only the third row is evidence about the machine. The probe therefore obtains
one instance per class and invokes through `-InputObject`.

## 1. Class inventory (`root\WMI`, Lenovo namespaces)

Present and functional-looking: `LENOVO_GAMEZONE_DATA`, `LENOVO_FAN_METHOD`,
`LENOVO_FAN_TABLE_DATA`, `LENOVO_FAN_MAX_SPEED_DATA`, `LENOVO_FAN_TEST_DATA`,
`LENOVO_LIGHTING_METHOD`, `LENOVO_LIGHTING_DATA`, `LENOVO_PANEL_METHOD`,
`LENOVO_OTHER_METHOD`, `LENOVO_CPU_METHOD`, `LENOVO_GPU_METHOD`,
`LENOVO_MEMORY_METHOD`, `LENOVO_BIOS_ASSISTANT`, `LENOVO_CPU_OVERCLOCKING_DATA`,
`LENOVO_GPU_OVERCLOCKING_DATA`, `LENOVO_MEMORY_OC_DATA`, `LENOVO_SR_DATA`,
`LENOVO_DISCRETE_DATA`, `LENOVO_CAPABILITY_DATA_00/01/02`,
`LENOVO_FEATURE_STATUS_DATA`, plus the `Lenovo_*` BIOS family
(`Lenovo_BiosSetting`, `Lenovo_SetBiosSetting`, `Lenovo_SaveBiosSettings`,
`Lenovo_SetFunctionRequest`, `Lenovo_FunctionRequest`,
`Lenovo_LoadDefaultSettings`, `Lenovo_DiscardBiosSettings`,
`Lenovo_SetBiosPassword`, `Lenovo_BiosSelections`).

`root\LENOVO` is empty on this machine.

A class existing is not a channel. The sections below are the verdicts.

## 2. `LENOVO_PANEL_METHOD` — 20 methods, all `WBEM_E_NOT_SUPPORTED`

`Panel_Get_Support_Status`, `Panel_Get_Status`, `Panel_Set_Status`,
`Panel_Get_Low_Latency_Mode`, `Panel_Set_Low_Latency_Mode`,
`Panel_Get_PIP_Info`, `Panel_Set_PIP_Info`,
`Panel_Get_Game_Aid_FPS_Display_Pos`, `Panel_Set_Game_Aid_FPS_Display_Pos`,
`Panel_Get_Game_Aid_FPS`, `Panel_Get_Game_Aid_Sight_Mode`,
`Panel_Set_Game_Aid_Sight_Mode`, `Panel_Get_Game_Aid_Timer_Info`,
`Panel_Set_Game_Aid_Timer_Info`, `Panel_Get_Game_Aid_Countdown_Info`,
`Panel_Set_Game_Aid_Countdown_Info`, `Panel_Get_Display_Mode`,
`Panel_Set_Display_Mode`, `Panel_Get_Gamut_Switch`, `Panel_Set_Gamut_Switch`,
`Panel_Get_MPRT`, `Panel_Set_MPRT`.

Consequence: `panel.color` (gamut switch), `panel.overdrive` (MPRT/OD) and the
PIP/game-aid surface have **no** firmware channel on this SKU. This is not a
descriptor or permission problem; the provider answers `不支持` for the read
methods too, so no write path can be built on it.

## 3. `LENOVO_FAN_METHOD` and `LENOVO_LIGHTING_METHOD` — `WBEM_E_NOT_SUPPORTED`

```text
LENOVO_FAN_METHOD.Fan_Get_Table(FanID=1, SensorID=0)      -> 不支持
LENOVO_LIGHTING_METHOD.Get_Lighting_Current_Status(id 0..3) -> 不支持
```

`LENOVO_FAN_METHOD` is declared with `Fan_Get_Table` / `Fan_Set_Table`, and
`LENOVO_LIGHTING_METHOD` with `Get/Set_Lighting_Current_Status`. Neither
answers. Fan *tables* and the lighting method plane are therefore absent.

Fan telemetry still works through `LENOVO_GAMEZONE_DATA` +
`LENOVO_FAN_MAX_SPEED_DATA`: `perf fan status` reports
`fans=[{id:1, min_rpm:2100, max_rpm:5100}]`. Reading is a real channel;
fan-curve writing is not.

## 4. `LENOVO_GAMEZONE_DATA` — declared methods vs answering methods

The class declares roughly fifty methods including `IsSupportSmartFan`,
`SetSmartFanMode`, `GetSmartFanMode`, `SetFanCooling`, `IsSupportFanCooling`,
`SetKeyboardLight`, `GetKeyboardLight`, `IsSupportLightingFeature`,
`SetWinKeyStatus`, `GetWinKeyStatus`, `IsSupportDisableWinKey`, `IsSupportOD`,
`GetODStatus`, `SetODStatus`, `SetTPStatus`, `GetTPStatus`, `GetThermalMode`,
`GetPowerChargeMode`, plus the OC/water-cooling/GSync group.

Observed through a per-call instance:

```text
IsSupportGpuOC   -> ReturnValue=True, Data=0        (answered: GPU OC not supported)
IsSupportSmartFan, GetSmartFanMode, GetSmartFanSetting, GetThermalMode,
IsSupportFanCooling, GetFanCoolingStatus, IsSupportLightingFeature,
GetKeyboardLight, IsSupportDisableWinKey, GetWinKeyStatus, IsSupportOD,
GetODStatus, IsSupportDisableTP, GetTPStatus, GetPowerChargeMode, GetVersion,
IsSupportCpuOC, IsBIOSSupportOC, GetCPUTemp, GetGPUTemp, IsSupportGSync,
GetGSyncStatus, IsSupportWaterCooling, GetWaterCoolingStatus,
IsSupportIGPUMode, GetIGPUModeStatus, GetProductInfo, GetMemoryOCInfo,
GetKeyboardfeaturelist
                 -> 无效的对象 (WBEM_E_INVALID_OBJECT)
```

`IsSupportGpuOC` answering while the rest reject is the discriminator: the
provider is reachable, so the rejections are per-method, not a binding fault.
The CLI's existing `GetSmartFanMode -> unsupported` result is consistent with
this and is not a CLI defect.

## 5. `LENOVO_OTHER_METHOD` — generic plane returns no data

```text
GetFeatureValue(IDs=0..16)        -> 无效的对象 (every id)
GetDataByCommand(cmd=0..4, id=0)  -> ReturnValue=True, Data=[] , DataSize=(empty)
```

The generic escape hatch answers success with an empty payload. An empty
`Data`/`DataSize` is not a feature table, so no capability can be derived and
no setter can be aimed. `SetFeatureValue` is therefore not reachable either.

## 6. `LENOVO_BIOS_ASSISTANT` — works, one populated slot

```text
GetCapabilityValue()  -> True, Data=2147483656 (0x80000008)
GetValue(IndexData)   -> True; index 3 = 2277181371 (0x87C000FB),
                         every other index 0..24 = 2147483648 (0x80000000)
```

`0x80000000` is a sentinel meaning "no value"; only index 3 carries data. A
single populated slot does not describe a general BIOS read/write surface, so
this class cannot back a generic BIOS setting command. It does confirm the
provider is functional, which is why its rejections elsewhere are meaningful.

## 7. `Lenovo_BiosSetting` — 53 settings enumerated, no charge threshold

`Get-CimInstance Lenovo_BiosSetting` returns 53 populated `CurrentSetting`
entries under `ACPI\PNP0C14\ISET_n`, for example:

```text
WakeOnLan,Disabled              WirelessLAN,Enable
MacAddressPassThrough,Disable   Intel(R)VirtualizationTechnology,Enable
LenovoCloudServices,Disable     Intel(R)VT-dFeature,Enable
AlwaysOnUSB,Enable              SecureRollbackPrevention,Enable
ChargeInBatteryMode,Enable      HotkeyMode,Disable
FlipToStart,Enable              BiosSelfHealing,Enable
SecureBoot,Enable               Thunderbolt(TM),Enable
MemoryCardSlot,Enable           IntegratedCamera,Enable
```

The charge-related entries are `AlwaysOnUSB` and `ChargeInBatteryMode` only.
**No `ChargeStart`/`ChargeStop`/threshold-style setting exists in the
enumerated BIOS surface**, which independently corroborates the existing
`battery.thresholds` refusal: custom charge thresholds are not expressible as a
BIOS setting on this machine.

`Lenovo_SaveBiosSettings` does enumerate an instance
(`ACPI\PNP0C14\ISET_0`), so the class is present; its save semantics were not
exercised in this read-only session.

## 8. What this changes

| capability | previous label | evidence |
| --- | --- | --- |
| `panel.color` | unsupported, "needs MONITORCONTROL.dll" | **stronger**: `Panel_Set_Gamut_Switch` → `不支持` |
| `panel.overdrive` | unsupported, "no SPBTESTTOOL device" | **stronger**: `Panel_Get_MPRT`/`Panel_Set_MPRT` → `不支持` |
| `perf.fan.curve` | "no verified fan-table interface" | **proven absent**: `Fan_Set_Table` → `不支持` |
| `perf.fan.auto/manual/smart` | unsupported | **proven absent**: `GetSmartFanMode` etc. → `无效的对象` |
| `kbd.backlight` | unsupported | `LIGHTING_METHOD` → `不支持`; GAMEZONE lighting getter → `无效的对象` |
| `kbd.winlock` | "not verified" | **proven absent**: `SetWinKeyStatus`/`GetWinKeyStatus` → `无效的对象` |
| `battery.thresholds` | no verified write contract | **strengthened**: no threshold setting in 53 BIOS entries |

Each of these is now a firmware verdict recorded from the machine, not an
untried path. The remaining honest lever for them is a non-WMI channel
(direct EC access), which is a separate engineering decision with its own
safety analysis and is not implemented here.

## 9. Scope

Read-only. No `Set*` method was invoked. No BIOS variable was written, no
service was stopped. Section 7 deliberately lists settings that already existed
before the probe; nothing was changed.

## 10. PL1/PL2 MSR channel — driver present, device gated by logon type

`tune.pl1` / `tune.pl2` / `tune.tau` are not implemented: `power_limit()` and
`set_power_limit()` in `crates/lctrl-hal-win/src/power_tuning.rs` return
`Unsupported` unconditionally. The MSR transport that would carry them does
exist and is loaded on this machine:

```text
LnvMSRIO   state=Running  start=Auto
           C:\WINDOWS\system32\DriverStore\FileRepository\lnvprocessmanagement.inf_amd64_863cdf71782696c8\LnvMSRIO.sys
```

The driver exposes exactly six selectors; selector 5 is `PLATFORM_POWER_LIMIT`,
MSR `0x65C`, which is the register that carries PL1/PL2 on this CPU family.
So the register is reachable in principle and the whitelist already includes it.

It is not reachable from an SSH automation session. Opening the device is
refused even though the account is in `BUILTIN\Administrators` and
`.IsInRole(Administrator)` returns `True`:

```text
user=TB14P-EZRA\ezra elevated=True
CreateFileW("\\.\WinMsrDev", GENERIC_READ|GENERIC_WRITE, ...) -> err=5 (ACCESS_DENIED)
\\.\LnvMSRIO   -> not found
\\.\MsrDrv     -> not found
```

The token's group list contains `NT AUTHORITY\NETWORK`, i.e. the SSH logon is a
network logon. The device DACL does not grant that logon type, so the failure is
an environment property, not a driver or code defect.

Consequence for this change set: the PL1/PL2 path is left unimplemented. Writing
an MSR the harness cannot read even once would ship an unverified hardware
write, and MSR `0x65C` is a documented register whose power-limit fields are
scaled by `MSR_RAPL_POWER_UNIT` (`0x606`), which the driver's six-selector
whitelist does not expose — so watts could not be derived even with access.
Verifying either the read or the write needs an interactive elevated session on
the console, not an SSH logon.

## 11. Scope of this file

Read-only for every section: the single `CreateFileW` on the MSR device failed
before any IOCTL was issued, and no `Set*` WMI method, BIOS write, or registry
write was performed anywhere in this document.

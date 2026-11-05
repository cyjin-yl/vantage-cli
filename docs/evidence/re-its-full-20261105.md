# ITS PowerSlider schema — full enumeration — 2026-11-05

Owner: `LNVDispatcherService.exe` (`LenovoProcessManagement` service), a C++
named-pipe RPC server (`CDispatchCommand`/`CNamedPipeServer`, validated client
connect via `GetNamedPipeClientProcessId`). All live state is mirrored under
`HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`
— the registry IS the readable contract even where the pipe framing is opaque.

## Read surface (live values, 21VG)

| value | live | role |
|---|---|---|
| `ITS_CurrentSetting` | 3 | requested ITS tier (we write this when dispatcher-free) |
| `ITS_CurrentSettingV` | 3 | dispatcher-confirmed echo of the tier |
| `ITS_AutomaticModeSetting` | 7 | auto sentinel — dispatcher owns ITS_CurrentSetting when 7 |
| `ITS_FanMode` | 4 | smart-fan tier; mirrors GetSmartFanMode (registry fallback) |
| `ITS_SE_AutoModeCapability` | 5 | scenario-engine auto capability mask |
| `ITS_SE_VantageSetting` | 1 | Vantage-sourced scenario setting |
| `ITS_SE_Scenario_0..6` | 0 | scenario-engine slot values |
| `ITS_SE_CurrentScenario` / `ITS_SE_HWCapability` | 0 / 48 | active scenario + HW mask |
| `ITS_60nits` / `ITS_UserNits` | 40 / 100 | brightness caps for ITS dimming |
| `ITS_FN_Capability` / `ITS_HW_Capability` | 4122 / 16 | FN + hardware feature masks |
| `PowerSlider` / `Enable` / `Version` / `Project_Version` | 1/1/8193/109 | enable + schema versions |

## Policy flags (control dispatcher ownership/behaviour)

| value | live | role |
|---|---|---|
| `Policy_ModernITS` | 1 | Modern-ITS owner asserted → blocks manual tier write |
| `Policy_Override` | 3 | override policy level |
| `Policy_ACDC` / `Policy_ACDCTag` | 0 / 1 | AC/DC profile selection |
| `Policy_DisablePLx` / `Policy_DynamicPLx` | 1 / 12 | PLx disabled + dynamic-PL mask |
| `Policy_PLxSetMode` | 65537 | packed u16 union (PLx mode per source) |
| `Policy_FNSyncPowerMode` | 1 | FN key syncs OS power mode |
| `Policy_NotifyBIOS` / `Policy_NamedPipe` | 1 / 1 | BIOS notify + named-pipe RPC enabled |
| `Policy_SWOEMV` / `Policy_SMBOption` | 3 / 65793 | OEM/supervisor packed options |
| `Policy_TurboTime` / `Policy_TurboValue` / `Policy_EPPTurbo` | 134744072 / 20 / 553934735 | turbo timing/limit/EPP packed fields |
| `Policy_DispatcherFunc` / `Policy_EnableFunc` / `Policy_PerformanceFunc` | 86037777 / 1049920 / 257 | dispatcher feature masks |
| `Policy_AIEngine` / `AIGC_Priority` | 0 / 2 | AI-engine enable + AIGC priority |

## Packed curve/power tables (string lists `key:val;key:val;…`)

| value | live shape | role |
|---|---|---|
| `Policy_PowerBucket` | `0:15;10:30;25:45;38:50;40:70;60:80;70:100;` | power-percent → bucket map |
| `Policy_CFQC` / `Policy_CFQC1` | `0:20;…;0:50;` / `0:20;…;0:60;` | fan-quiet curve tables (two profiles) |
| `Policy_CPPC` | `50:80;50:72;…30:50;` | CPPC autonomous-mode hint table |
| `Policy_BenchWA` / `Policy_NotRuninAC` / `Policy_Send2ServOption` | 33346 / 16777473 / 1 | benchmark/not-on-AC/service packed options |

## Commands recovered from the binary (named-pipe RPC verbs)
`GetSmartFanMode`, `SetSmartFanMode`, `SetFanTale` (with `_TryAgain`),
`GetNamedPipeClientProcessId` client validation. `Policy_APMinSTDFan`,
`Policy_APMinSTDFan_CPU`, `Policy_APMinSTDFan_GPU`, `Policy_FanSpeedAddress`
are fan-speed policy/address values.

## What this unlocks vs. what's still opaque
- `ITS_CurrentSetting` (perf tier) — **implemented** (12d77ec): registry write,
  gated on auto!=7 + ModernITS!=1, readback + rollback.
- `ITS_FanMode` (smart-fan tier) — **read fallback implemented**: when the
  GAMEZONE `GetSmartFanMode` WMI returns out-of-domain (it does on 21VG), read
  `ITS_FanMode` instead of reporting unavailable.
- `SetSmartFanMode`/`SetFanTale` over the named pipe — the framed RPC protocol;
  the registry mirror is read-safe but writes are better routed through the
  existing GAMEZONE `SetSmartFanMode` WMI verb (already wired) than the opaque
  pipe framing. Pipe framing reversal is future work.
- `Policy_*` curve tables — documented; writing them is firmware-risky and not
  yet implemented.

Target scratch (not committed): `itsfull.txt`, `itsfan.txt`, `pipename.txt`,
`fanenum.txt` under `C:\Users\ezra\`.

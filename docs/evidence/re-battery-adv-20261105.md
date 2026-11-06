# Charge-firmware modes beyond normal/conservation/rapid — owner, enums, write contracts — 2026-11-05

Scope: `battery extreme-life`, `battery night-charge`, `battery temporary-mode`, and any
charge mode the vendor firmware exposes beyond the two implemented GBMD modes. Read-only
static analysis of the installed target binaries plus the already-retained live probe
output. Recon on the target was limited to file copies (`scp`), file/WMI inventory and
class enumeration (PowerShell); no hardware write, registry write, service change, or
build/run of any program was performed for this document.

## 1. Owner resolution (the WMI hypothesis is disproved)

| candidate | result |
|---|---|
| WMI `Lenovo_SetBatteryChargeMode` / `LenovoSmartBattery` | **does not exist on 21VG**: `root/WMI` has 65 Lenovo classes and no charge-mode method class; the only charge-related WMI names are the read-only event `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT`, the empty `Lenovo_BatteryInformation`, and the OS `Battery*` classes. `root/Lenovo` is not a namespace (`__NAMESPACE` enumeration: subscription, DEFAULT, CIMV2, msdtc, Cli, Intel_ME, SECURITY, HyperVCluster, SecurityCenter2, RSOP, PEH, StandardCimv2, WMI, MSPS, directory, Policy, virtualization, Interop, Hardware, ServiceModel, SecurityCenter, Microsoft, Appv). |
| `LenovoSmartBattery` module | no such binary; `SmartBattery` appears only as `IsSupportSmartBatteryV2`/`GetSmartBatteryV2Info`/`SmartBatteryInfo` (SMB telemetry struct), not a mode writer. |
| real owner | `PowerBattery.dll` 1.0.13.114, class `CChargingMode` (native x64, ordinal exports), over `\\.\EnergyDrv` (driver `AcpiVpc.sys`, 4-byte selector passthrough). Contract layer above it: `IdeaBatteryAgent.dll` (mixed-mode .NET/C++/CLI, `BatteryAgent`) and the wire enums in `BatteryManagementContract.dll`. |
| PCManager UI | `skin/html/app/device/js/PCM_chargemode.js` renders a **4-state radio** `{0 常规模式, 1 养护模式, 2 智能快充, 3 夜间优化}` and sends the id back through `NEWPLUGIN_WRAPPULGIN_SETBATTERYCHARINGMODE`. State 3 is **night charge**, i.e. the UI merges two firmware features into one radio group; it is not a fourth GBMD mode. `plugins/PCMBatterySettingsPlugin.dll` only logs `GetBatteryChargingMode mode = %d`, `SetBatteryChargingMode mode = %d`, `GetBatterySupportChargingType type = %d` and routes to the iMController contract `SOFTWARE\Lenovo\iMController\Contracts\SystemManagement.BatteryMgmt`. |

That plugin's own helper set is `CStorageChargingHelper` / `CQuickChargingHelper` /
`CDriverHelper` (`BatteryChargingModeHelper` RTTI) — there is no night-charge or
temporary-mode helper class, so the UI's fourth radio state is fed by the WRAPPULGIN
support map, not by a fourth firmware selector.

Artifacts analysed (copied read-only from the target, hashes of the exact bytes):

| file | sha256 |
|---|---|
| `C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\1.0.13.114\PowerBattery.dll` (145,376 B) | `df7a014a15630dbfa36603014e15b8b729ec500840b6f45cfb853bb957c10e91` |
| `…\IdeaNotebookAddin\1.0.13.114\IdeaBatteryAgent.dll` (285,664 B) | `859a5f29ba56c1399651159bf3a42eb6b5d516d3532f436aeffc52fad2223fa8` |
| `…\IdeaNotebookAddin\1.0.13.114\BatteryManagementContract.dll` (54,240 B) | `f5d3ced0caa8e344d198ffede87fd3c35446aa7263f231c1850bf9a4bf600d3c` |
| `C:\Program Files (x86)\Lenovo\PCManager\5.1.210.10222\plugins\PCMBatterySettingsPlugin.dll` (202,504 B) | `2897628dc5393f5d52aa3950f42e2394119078b8b0bb7b34a2375784961a4659` |
| `…\PCManager\5.1.210.10222\skin\html\app\device\js\PCM_chargemode.js` (2,316 B) | `2a609c09d401d7380187baad0b0f5b457ccc615fad46687e69d7d0bbec2693e0` |

## 2. Recovered mode enums (exact values, not inferred from names)

`BatteryManagementContract.dll` enum literals (`monodis`, `.field public static literal`):

| enum | values |
|---|---|
| `BatteryChargeModeType` | `Normal=0`, `Storage=1`, `Quick=2` |
| `BatteryNightChargeModeType` | `NotSupported=0`, `On=1`, `Off=2` |
| `BatteryTemporaryModeType` | `None=0`, `Permanent=1`, `PlugIn=2`, `DynamicLifeSpan=4` |
| `SupportedBatteryChargeModeType` (bitmask) | `None=0`, `Normal=1`, `Storage=2`, `Quick=4` — **no** storage80/limited bits |
| `CapabilityType` (bitmask) | `BatteryChargeMode=0x08`, `ChargeModeQuick=0x10`, `ChargeModeNormal=0x20`, `ChargeModeStorage=0x40`, `ChargeModeStorageLimited=0x100`, `ChargeModeStorage80=0x200` |

Consequence: `ChargeModeStorageLimited`/`ChargeModeStorage80` are **capability bits, not
write modes**; `SupportedBatteryChargeModeType` has no value for them, and the agent
rejects any mode numbering above 3 (`BatteryAgent::FillResponseWithCurrentChargingMode`:
`ldc.i4.3 / bgt.un → … "unknown charge mode"`, then decodes bit1→Quick and bit0→Storage).

Native side (`PowerBattery.dll` export name table → RVA):

The managed layer passes the contract enums through **without remapping**
(`IdeaBatteryAgent.dll` IL): `SetBatteryChargeMode` checks `mode <= 3` and then calls
`CChargingMode.SetChargingMode(mode)` unchanged (values above 3 log
`"SetBatteryChargeMode received invalid charge mode: "`), and
`SetBatteryNightChargeMode` accepts `1..=2` and calls `SetNightChargeMode(mode)`
unchanged. There is no separate "temporary mode" setter in the contract either.

| export | RVA |
|---|---|
| `CChargingMode::GetChargingMode` | `0x9bc0` |
| `CChargingMode::SetChargingMode` | `0x9c30` |
| `CChargingMode::SetChargeModeRegistry` | `0xa0d0` |
| `CChargingMode::DoesSupportRapidChargeMode` | `0xa110` |
| `CChargingMode::DoesSupportConservationMode` | `0xa130` |
| `CChargingMode::DoesSupportTemporaryMode` | `0xa250` |
| `CChargingMode::GetTemporaryMode` | `0xa3d0` |
| `CChargingMode::DoesSupportNightChargeMode` | `0xa5c0` |
| `CChargingMode::GetNightChargeMode` | `0xa770` |
| `CChargingMode::SetNightChargeMode` | `0xa930` |
| `CChargingMode::DoesSupportStorage80` | `0xaa80` |
| `CChargingMode::GetEmSpecVersion` | `0xabc0` |
| `CBatteryDriver::GetEmSpecVersion` | `0x22d0` |
| `CChargingMode::EM_SPEC_VERSION_STORAGE_80` (.data) | `0x21174` (value `0xb8` = 184) |

## 3. Write contracts

### 3.1 GBMD `0x831020f8` (1-byte subcommand) — charge mode, **exactly three modes**

`SetChargingMode(mode)` `0x9c30` dispatches on the parameter with `test edx,edx / je`
(mode 0 → RVA `0x9ea8`), `sub ecx,1 / je` (mode 1 → `0x9d8b`), `cmp ecx,1 / jne 0x9fde`
(mode 2 → `0x9c65`, anything else → the invalid branch `0x9fde`). There is no mode 3/4 path.
Subcommands are unchanged from `docs/interfaces.md` §11.1: `03`/`05` for storage,
`0d`/`0f` appended only for subtype 2, `07`/`08` for express, `ff` for the status bitmask.
`GetChargingMode` `0x9bc0` returns `1` (storage vtable slot `+0x18` and `+0x20` both 1),
else `2` (express `+0x18`/`+0x20`), else `0`.

### 3.2 BNMC `0x83102150` (4-byte selector) — night charge, `On`/`Off` only

| step | selector | output | decode |
|---|---|---|---|
| `DoesSupportNightChargeMode` (`0xa5c0`) | `0x13` | 4 B | supported iff `(out & 1) != 0 && ((out >> 4) & 1) != 0` |
| `GetNightChargeMode` (`0xa770`) | `0x11` | 4 B | `0` when the support probe failed; else `2` when `out & 0x10 == 0`, `1` when set → `On=1, Off=2` (`BatteryNightChargeModeType`) |
| `SetNightChargeMode` (`0xa930`) | `0x80000012` = On, `0x12` = Off | 4 B (zero-init) | `sub ecx,1 / je` → On; `cmp ecx,1 / jne` → invalid; after the IOCTL it polls `GetNightChargeMode` up to 10× with `Sleep(50)` and returns the polled value |

Live target already recorded (`docs/evidence/storage80-aiturbo-20261102.txt`): probe
`0x11` (bit0 set, bit4 set → supported), status `0x01` (bit4 clear → off). The HAL
implementation (probe `0x13`, status `0x11`, writes `0x80000012`/`0x12`, 10×50 ms
readback) matches this contract.

### 3.3 BTMC `0x83102148` (4-byte selector) — temporary mode, **read-only**

| step | selector | output | decode |
|---|---|---|---|
| `DoesSupportTemporaryMode` (`0xa250`) | `0x13` | 4 B | true iff `(out & 1) != 0 && (out & 0x6) != 0`; false otherwise (invalid reply → 0) |
| `GetTemporaryMode` (`0xa3d0`) | `0x11` | 4 B | returns `0` if the probe failed; if `(out & 1) == 0` → `0` (`None`) and logs `out & 0x0e`; else `v = (out >> 4) & 0x07ffffff` → `0` = `Permanent(1)`, `1` = `PlugIn(2)`, `2` = `DynamicLifeSpan(4)`, any other `v` → `0` |

There is **no writer**: the export table has no `SetTemporaryMode`; the only other
`0x83102148` uses in the whole binary are the two above (`0x60a0`, `0x6230`, `0x63e0`,
`0x6590` are an unrelated, unreferenced 4-query helper block using selectors
`0x23`/`0x21`/`0x31`/`0x41`, whose struct filler `0x6050` has no caller in this image).
Live target: `0x83102148/0x13 → 3` (bit0 set, bit1 set → supported/active), `0x11 → 1`
(bit0 set, `v = 0` → `Permanent`).

Correction to earlier notes: the response word is **not** a bitmask `{bit0 Permanent,
bit1 PlugIn, bit2 DynamicLifeSpan}`. Bit0 is a validity flag and the mode lives in bits
4..26; the old bitmask reading happens to agree only for words `0x01`/`0x02`/`0x04`.

### 3.4 Storage80 / StorageLimited — capability predicates, no separate write

`DoesSupportStorage80` (`0xaa80`) = `GetEmSpecVersion() >= [0x180021174] (184)` **and**
GBMD `0xff` bit23 (`shr edi,0x17 / and edi,eax`); it returns the bit, and neither it nor
any other function has a second storage subcommand. `ChargeModeStorageLimited` is only a
contract capability bit with no corresponding method anywhere in
`IdeaBatteryAgent.dll`/`PowerBattery.dll`.

### 3.5 Extreme life — not a charge-firmware mode

Owner is the AiTurbo stack (`BatteryLife.exe` / `AiPerfSvc.exe` / `LseFileMonitor.sys`),
already fully documented in `docs/evidence/extreme-life-re-20261102.md`: a versioned
run-state policy (power-scheme DC indices, ITS source, EcoQoS, job CPU cap, window
actions) keyed in `HKCU\Software\Lenovo\AiTurbo\BatteryLife`. No GBMD/BNMC/BTMC selector
carries it, so it cannot be implemented as a charge mode; it stays `Unsupported` until an
owned signed kernel Job lifecycle exists.

## 4. Reachable mode set (conclusion)

| feature | writable on 21VG | channel | evidence |
|---|---|---|---|
| Normal / Conservation(Storage) / Rapid(Quick) | yes (already implemented) | GBMD `0x831020f8` `03/05[/0d/0f]`, `07/08` | §3.1 |
| Night charge On/Off | yes (`battery night-charge`) | BNMC `0x83102150` `0x80000012`/`0x12` | §3.2 |
| Storage80 / StorageLimited | not a mode | capability bits only | §3.4 |
| Temporary mode (Permanent/PlugIn/DynamicLifeSpan) | **no writer exists** → read-only probe | BTMC `0x83102148` `0x13`/`0x11` | §3.3 |
| Extreme life | not firmware | AiTurbo policy | §3.5 |

Deliberate non-change: `battery charge-mode` keeps its three-value enum
(`normal|conservation|rapid`). PCManager's radio adds `3 = night`, but that value maps to
the **separate** BNMC register, and no evidence shows that setting it clears or is cleared
by the GBMD Storage/Express bits (the vendor's own `SetBatteryChargingMode` forwards the
number without any storage/express transition). Night stays its own command
(`battery night-charge on|off`), whose write/readback/rollback already exists.

## 5. Source corrections made with this evidence

- `crates/lctrl-core/src/battery.rs`: `TemporaryChargeState` now mirrors the native decode
  (`supported` from the `0x13` probe, `valid` = bit0 of the `0x11` reply,
  `mode: Option<TemporaryChargeMode>` with `Permanent=1/PlugIn=2/DynamicLifeSpan=4`),
  replacing the bitmask interpretation; `NightChargeState::from_probe` now requires
  `bit0 && bit4`, matching `DoesSupportNightChargeMode` instead of bit0 alone.
- `crates/lctrl-hal-win/src/ioctl_contract.rs` + `p0.rs`: added the BTMC `0x13` probe and
  build the state from probe+status; IOCTL failures remain `ChannelUnavailable`.
- `crates/lctrl-hal-win/src/windows_hal.rs`: the `battery.temporary-mode` capability now
  probes `0x83102148/0x13` (was `0x11`) and names that selector in the unavailable reason.
- `crates/vantage-cli` `battery temporary-mode` output and the
  `examples/charge_domain_probe.rs` printer follow the new fields.
- `crates/lctrl-hal/src/command_capability.rs`: `BATTERY_EXTREME_LIFE` unsupported reason
  now names the AiTurbo owner and states that the PCManager radio's extra state is night
  charge, not a fourth firmware mode.
- Docs: `docs/interfaces.md` §11.3/§11.4 + the "11.4 修正" block (per-function RVA table,
  the `0x23/0x21/0x31/0x41` block re-scoped as unreferenced), `docs/02-power-battery.md`
  §5/§13, `docs/A-wmi-reference.md` (no charge-mode WMI class), `docs/C-acceptance.md` C26
  (addendum preserving the verbatim 2026-11-01 output).
- `crates/lctrl-core/tests/battery_contract.rs`: decoder tests for the night support
  predicate (`0x11`/`0x10`/`0x01`) and the temporary mode field (`0x01`/`0x12`/`0x21`/
  `0x04`/`0x31`), the `is_temporary` split, and the contract value/`serde` names.

## 6. Not verified here (parent acceptance)

No target build/run was performed for this document. Recommended checks: (a)
`battery temporary-mode` on 21VG must print `supported=true valid=true mode=permanent`
from the retained live words (`0x13 → 3`, `0x11 → 1`); (b) a machine/path returning
`0x11 → 0x12` must report `plug_in`; (c) `0x11 → 0x04` (bit0 clear) must report
`valid=false mode=null` instead of the old `dynamic_lifespan=true`; (d) `battery
night-charge off` on an already-off machine must be idempotent and
`battery night-charge on` must still read back `enabled=true`; (e) `battery capability`
must keep `battery.extreme-life` Unsupported while `battery.temporary-mode` stays
read-only.

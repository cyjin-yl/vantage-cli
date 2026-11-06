# Lenovo BIOS config provider and read contract — 21VG — 2026-11-05

Target: TB14P-EZRA, ThinkBook 14 G8+ (21VG, Panther Lake), Windows 11 build
26100. Session scope: **read-only**. No `Set*`/`Save`/`Discard`/
`LoadDefaultSettings`/`SetBiosPassword` method was invoked, no registry value,
service or file was written.

Question this answers: the assignment premise says
`bios.discard` / `bios.defaults` / `bios.password.clear` /
`bios.password.verify` are `Unsupported` and that BIOS control is a
"`LenovoBiosConfig` / SVP" WMI interface; implement `bios config read <setting>`
if the WMI class exists.

Probe scripts and raw output on the target (all read-only):

```text
C:\Users\ezra\bc-wmi.ps1        -> C:\Users\ezra\biosconfig-probe\wmi.txt
C:\Users\ezra\bc-strings.ps1    -> ...\strings.txt        (first-pass sweep, 900 s)
C:\Users\ezra\bc-strings2.ps1   -> ...\strings2.txt       (targeted, IndexOf-based)
C:\Users\ezra\bc-iface.ps1      -> ...\biosiface-strings.txt
C:\Users\ezra\bc-classes.ps1    -> ...\classes.txt
C:\Users\ezra\bc-settings.ps1   -> ...\settings.txt
```

## 1. Provider hunt — `LenovoBiosConfig` does not exist on this SKU

| probe | result |
| --- | --- |
| `root` child namespaces | 22 namespaces; **no `root\Lenovo`** (`root\Lenovo\BiosConfig` MISSING) |
| `root\WMI` class census | 1650 classes; no `LenovoBiosConfig`, `Lenovo_BiosConfig`, `Lenovo_GetBiosSetting`, `Lenovo_BiosSelections` |
| `__Win32Provider` in `root\WMI`, `root\CIMV2`, `root\StandardCimv2` matching `lenovo\|bios\|acpi\|svp` | **empty** — no Lenovo WMI provider is registered |
| binary string sweep, 3161 files under `ProgramData\Lenovo\Vantage\Addins`, `Program Files (x86)\Lenovo\VantageService`, PCManager `plugins`+`Modules`, and the two Lenovo DriverStore packages | **zero** hits for `LenovoBiosConfig` or `BiosConfig` |

What the name actually refers to:

| identifier | where it lives | what it is |
| --- | --- | --- |
| `LenovoBiosWmiInterface` | ASCII string in `C:\Program Files (x86)\Lenovo\VantageService\5.1.2609.9\Lenovo.Vantage.ConfigService.dll` (also `RootWmi`, `Lenovo.Vantage.ConfigService.WMITag`) | the Vantage ConfigService's internal WMI-backed BIOS interface name — **not** a WMI class |
| `WmiAgent` / `BiosAgent` / `WmiClassLenovoBiosSetting` / `WmiClassGetBiosSelections` | `C:\ProgramData\Lenovo\Vantage\Addins\LenovoProductivitySystemAddin\1.0.0.138\LenovoProductivitySystemAddin.dll` | the only Lenovo component that actually consumes the BIOS classes |
| `SupervisorPassword` | ASCII data-field name in `LenovoSnapshotAddin\2.2.26070.2\Lenovo.Cdat.dll` / `Lenovo.Cdat.Core.dll` | snapshot metadata field; no SVP WMI class exists |

The provider of the BIOS classes is Microsoft's WMI-ACPI mapper, not a Lenovo
driver:

```text
device      ACPI\PNP0C14\ISET   "Microsoft Windows Management Interface for ACPI"  Status=OK
siblings    DSARDEV, LSK20, WBAT, ASIS, ATAG, TESTDEV, LSR, GMZN (all OK)
qualifier   Lenovo_BiosSetting: dynamic=True, WMI=True, provider=WMIProv,
            GUID={51F5230E-9677-46cd-A1CF-C0B23EE34DB7}, WmiExpense=1
host DLL    C:\Windows\System32\wbem\wmiprov.dll (241664 bytes)
instances   53 x ACPI\PNP0C14\ISET_0 .. ISET_52
```

`provider=WMIProv` is the ACPI WMI mapper; the class bodies are ACPI control
methods on the `ISET` device. There is no Lenovo namespace, provider
registration, or SVP class to bind.

## 2. Live class shapes (`Get-CimClass`, this session)

```text
Lenovo_BiosSetting          props  Active:Bool CurrentSetting:String InstanceName:String
Lenovo_GetBiosSelections    method GetBiosSelections(Item:String, out Selections:String)
Lenovo_SetBiosSetting       method SetBiosSetting(parameter:String, out return:String)
Lenovo_SaveBiosSettings     method SaveBiosSettings(parameter:String, out return:String)
Lenovo_DiscardBiosSettings  method DiscardBiosSettings(parameter:String, out return:String)
Lenovo_LoadDefaultSettings  method LoadDefaultSettings(parameter:String, out return:String)
Lenovo_SetBiosPassword      method SetBiosPassword(Parameter:String, out Return:String)
Lenovo_BiosPasswordSettings props  Active InstanceName MaxLength MinLength
                                   PasswordMode PasswordState
                                   SupportedEncodings SupportedKeyboard   (all UInt32)
LENOVO_BIOS_ASSISTANT       methods GetCapabilityValue(out Data:U32)
                                   GetValue(IndexData:U32, out Data:U32)
                                   SetValue(IndexData:U32, ValueData:U32, out ReturnData:U32)
```

`Lenovo_LoadDefaultSettings` and `Lenovo_SetBiosPassword` are present with the
documented arity, so "the method class is missing" is **not** why any password
operation is refused (see §4).

## 3. The read contract (the only interface the firmware populates)

`SELECT CurrentSetting FROM Lenovo_BiosSetting`:

```text
instances        53
non-empty        34   (ISET_5/7/8/9/10/13/14/20/22/24/25/29/30/31/33/37/42/44/51 are "")
with a comma     34
distinct names   34, no duplicates
sample           [ISET_0]  WakeOnLan,Disabled
                 [ISET_16] HotkeyMode,Disable
                 [ISET_18] AlwaysOnUSB,Enable
                 [ISET_43] SecureBoot,Enable
                 [ISET_48] EFI-BootOrder,01:02
```

34 non-empty settings is exactly what `vantage bios list` returns on this
machine (`docs/evidence/target-bios-selections-20261030.json`), so the CLI read
path is not dropping populated rows.

`Lenovo_GetBiosSelections.GetBiosSelections(Item)` answers live:

```text
HotkeyMode -> "Enable,Disable"
WakeOnLan  -> "Disabled,AC Only,AC and Battery"
```

`Lenovo_BiosPasswordSettings` (one instance, `ACPI\PNP0C14\ISET_0`):

```text
MinLength=1 MaxLength=128 PasswordMode=1 PasswordState=0
SupportedEncodings=1 SupportedKeyboard=1
```

`LENOVO_REPORT_DIRECT_BIOS_DATA` — the typed direct-BIOS option table — has a
full schema but **zero instances**:

```text
Active default_num DependOnID DependOnValue DiscreteData Help InstanceName
LanguageID Max_Value Min_Value Name Option_help[] Option_name[] Option_num
Option_value[] RebootFlag Setup_ID Step
instance_count = 0
```

So there is a second, typed BIOS read surface declared in `root\WMI`, but this
SKU's firmware publishes no rows into it. It cannot back a named-setting read.

### Official consumer contract (what Lenovo itself calls)

UTF-16 strings in `LenovoProductivitySystemAddin.dll` (`WmiAgent`):

```text
root\WMI
SELECT * FROM Lenovo_BiosSetting
Lenovo_GetBiosSelections / GetBiosSelections / Selections
Lenovo_SetBiosSetting / SetBiosSetting
Lenovo_SaveBiosSettings / SaveBiosSettings
SELECT * FROM Lenovo_BiosPasswordSettings / PasswordState
LENOVO_BIOS_ASSISTANT   (GetBiosCapability, CheckBiosPasswordSet)
```

It contains **no** call to `DiscardBiosSettings`, `LoadDefaultSettings` or
`SetBiosPassword`, which matches `docs/05-bios-settings.md` §5.4 (official
software only reads password state).

## 4. What the repo already implements, and where the premise is stale

| capability id | row (`crates/lctrl-hal/src/command_capability.rs`) | CLI dispatch | live channel |
| --- | --- | --- | --- |
| `bios.list` | `CommandId::BIOS_LIST` row — Implemented | `BiosCommand::List` | 34 items read (§3) |
| `bios.get` | `CommandId::BIOS_GET` row — Implemented | `BiosCommand::Get` | per-setting read (§3) |
| `bios.set` | `CommandId::BIOS_SET` row — Implemented | `BiosCommand::Set` | staging contract verified on target 2026-11-03 (machine-global buffer); `SetBiosSetting` itself is firmware-refused — 2026-11-04 matrix records `"Not Supported"` for every setting |
| `bios.save` | `CommandId::BIOS_SAVE` row — Unsupported("standalone global BIOS-buffer save is not supported") | `BiosCommand::Save` → `Unsupported` | intentional |
| `bios.discard` | `CommandId::BIOS_DISCARD` row — **Implemented** | `BiosCommand::Discard` arm of `execute_bios` | `DiscardBiosSettings` → `Success` (2026-11-04) |
| `bios.defaults` | `CommandId::BIOS_DEFAULTS` row — **Implemented** | `BiosCommand::Defaults` arm | `LoadDefaultSettings` present; no target write acceptance |
| `bios.password.status` | `CommandId::BIOS_PASSWORD_STATUS` row — Implemented | `PasswordCommand::Status` | `PasswordState` read live |
| `bios.password.set` | `CommandId::BIOS_PASSWORD_SET` row — Unsupported("documented no-password-write boundary", `docs/interfaces.md` §0) | `PasswordCommand::Set`/`Clear` arm → `bios.password.write` | policy, not a missing method |
| `bios.password.clear` | `CommandId::BIOS_PASSWORD_CLEAR` row — Unsupported (same policy) | same arm | policy, not a missing method |
| `bios.password.verify` | `CommandId::BIOS_PASSWORD_VERIFY` row — **Implemented** | `PasswordCommand::Verify` arm | same-value `SetBiosPassword`; refused up front when `supervisor_set` is false |

So of the four capabilities named in the assignment, three are already
Implemented at this commit and one (`password.clear`) is a deliberate policy
refusal, not an unimplemented channel.

**Reporting gap (not a channel gap).** These rows bind
`FeatureBinding::Unprobed("bios")` / `"bios.discard"` / `"bios.defaults"` /
`"bios.password.*"`, but the Windows HAL records only
`bios.staging.contract` and `channel.bios`
(`WindowsHal::capabilities` in `crates/lctrl-hal-win/src/windows_hal.rs`
records them). No HAL ever records
a capability key named `bios`, `bios.discard`, `bios.defaults`,
`bios.password.clear` or `bios.password.verify`, so `feature_presence()`
(`crates/vantage-cli/src/capability.rs:988`) returns `Unverified` for every BIOS
row and `vantage capability` prints them as `unverified` — never `available`,
regardless of what the firmware answers. Re-binding those rows to the recorded
keys (and adding a read probe if the password rows are to be reported) is a
central-registry change left to the parent; it is deliberately not made here.

One nuance for the parent's decision: `bios.password.verify` is reported
read-only but is implemented by invoking the password **write** method with an
identical old/new value (`PasswordCommand::Verify` arm →
`crates/lctrl-hal-win/src/bios.rs::verify_password`). It is guarded by
`PasswordState` so it can never create a password on this machine
(`PasswordState=0`, §3), but it is still a password-class call. That is the
existing design, unchanged by this session.

## 5. Why no new `bios config read` command

- The WMI class the item conditions on (`Lenovo_BiosSetting` +
  `Lenovo_GetBiosSelections`) exists and is already surfaced as
  `vantage bios list` / `vantage bios get <name>` — the read commands named in
  the README capability table and exercised by the PR #42 read-only acceptance
  in `docs/C-acceptance.md`.
- No `LenovoBiosConfig`/SVP class, namespace or provider exists (§1), so there
  is nothing new to bind.
- A second command spelling for the same firmware read would duplicate a
  documented surface with no new capability.

Repair made instead (read-only, same channel): `WindowsBiosController::get` in
`crates/lctrl-hal-win/src/bios.rs` now fills `BiosItem.selections` from
`Lenovo_GetBiosSelections`. Previously only `list()` carried the value domain,
so `bios get <name>` returned a `BiosItem` whose `selections` was always empty —
indistinguishable from "this setting has no domain" and inconsistent with
`BiosItem` as returned by `list` and with the CLI contract test's `FakeBios`
(`impl BiosControl for FakeBios` in `crates/vantage-cli/tests/cli_contract.rs`).
A failing selections call is
tolerated exactly as `list()` tolerates it, so settings the firmware exposes
without a domain stay readable. Not executed here — build and test are the
parent's step.

## 6. Limits

- Read-only session: only `SELECT`-shaped queries and
  `GetBiosSelections`/property reads were issued. No `SetBiosSetting`,
  `SaveBiosSettings`, `DiscardBiosSettings`, `LoadDefaultSettings`,
  `SetBiosPassword` or `LENOVO_BIOS_ASSISTANT.SetValue` call.
- `bios.defaults` has still never been executed on hardware; its
  `Readback::Unverified` row remains accurate.
- `SetBiosSetting` write refusal is quoted from the 2026-11-04 matrix, not
  re-tested here (this session must not issue writes).
- `LENOVO_REPORT_DIRECT_BIOS_DATA`'s empty table is a snapshot from this boot;
  a firmware/BIOS update could populate it, in which case a typed read becomes
  possible.

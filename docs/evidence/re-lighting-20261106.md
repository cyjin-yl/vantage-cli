# 21VG lighting plane — definitive read classification (2026-11-06)

Target: TB14P-EZRA (Lenovo ThinkBook 14 G8+ IPH, 21VG),
Windows 11 build **26300.9711** (the machine has been updated since the
2026-11-05 note, which recorded 26100; the lighting result is unchanged).
Work item: LIGHTING (retry of `kbd.backlight`).

Question: does *any* lighting READ answer on this SKU, so that a lighting
channel can be implemented against it? Answer: **one does — the ACPI `KBLC`
state word behind `\\.\EnergyDrv` IOCTL `0x83102144` — and it is already the
implemented primary channel. No WMI lighting plane answers at all.** The four
`LENOVO_*` lighting surfaces are classified below with the exact HRESULT each
returns, so the "unsupported" label stops being an inference.

Scope: read-only. No `Set*` method was invoked (`SetKeyboardLight`,
`Set_Lighting_Current_Status`, `SetLightControlOwner` were deliberately
skipped), no registry value was written, no service was started or stopped,
and the KBLC device handle was opened with `GENERIC_READ` (a `GENERIC_READ|
GENERIC_WRITE` open was also exercised once, and only query/get IOCTLs were
issued through it).

## 1. Method

Four PowerShell probes, each written to a file on the target, executed with
`-File`, and its output saved next to it. Nothing is inferred from a class
merely existing: every verb below was invoked and its answer recorded.

Schema and error codes are read through `System.Management`
(`ManagementClass` / `ManagementObject.InvokeMethod`), not
`Get-CimInstance`/`Invoke-CimMethod`. That distinction matters twice:

- `Invoke-CimMethod` wraps every provider failure in `0x80131500` and loses
  the WBEM code, so it can only report the localized message (`不支持`,
  `无效的对象`). The WBEM code is what a Rust client sees.
- `Get-CimClass` reported **zero** methods for `LENOVO_LIGHTING_METHOD`,
  `LENOVO_LIGHTING_DATA` and `LENOVO_GAMEZONE_DATA` on this machine while
  `System.Management.ManagementClass` reported 2, 0 and 52 methods for the
  same classes. Class schema must be read through the classic API here.

Probe scripts (host, outside the repo) and target outputs:

```text
/tmp/lighting_probe.ps1    13a6d4c4c4c27bbb0823027218a4e99c691854331e6c9cd584dd5095085deb92
/tmp/lighting_probe2.ps1   e977214301f92932f234a2621c07344bfdef849a4425938bd32a76452f6e50cd
/tmp/lighting_probe3.ps1   877ca95981b1d1a717e3e462b69baa72ff168e453326ec40ee5dd20b88866bef
/tmp/lighting_probe4.ps1   9f4379adceec09e07c2b80e9806e76f29b3ff77837b78f576cd48471785bfd01
/tmp/lighting_probe5.ps1   8407d54bcb9a68749e36c1f2b1ec79563c59042caed0d6d67ae5177431529e60

C:\Users\ezra\lighting-probe-20261106.txt    7C3B73224735D92A647FD3485B9C30503C621A6E004D2865A68ADC59074314F8
C:\Users\ezra\lighting-probe2-20261106.txt   E3027BFAD23826A9DD2ECBD7B5C9BE587BFCEA2613B3CBD08A88C71AB75326D8
C:\Users\ezra\lighting-probe3-20261106.txt   C069A604032507FBF62929B768E4DCEBB82A652440F4F3CF9EA4350AD4B6C7E8
C:\Users\ezra\lighting-probe4-20261106.txt   AB9B57012AFA5DD55CB7B942AF59466571D8B7D256263250E3193AC98470364C
C:\Users\ezra\lighting-probe5-20261106.txt   29F32E9650FCDEC24807E64B1F5036354ADD097E45B5E7E707B3B1DEFEF06DB8
```

## 2. Result table

WBEM code meanings are fixed by `wbemcli.h` and confirmed here by the
provider's own message text: `0x8004100C` = `WBEM_E_NOT_SUPPORTED`
("Not supported"), `0x8004100F` = `WBEM_E_INVALID_OBJECT` ("Invalid object"),
`0x8004102F` = `WBEM_E_INVALID_METHOD_PARAMETERS` ("Invalid method
Parameter(s)").

| # | Plane / verb | Invocation shape | HRESULT | Message | Verdict |
| --- | --- | --- | --- | --- | --- |
| 1 | `LENOVO_LIGHTING_DATA` | `SELECT *` (all instances) | **0x8004100C** | Not supported | no instance: no read |
| 2 | `LENOVO_LIGHTING_DATA` | `SELECT * WHERE Lighting_Id = 0` | **0x8004100C** | Not supported | no read |
| 3 | `LENOVO_LIGHTING_METHOD` | instance enumeration | **0x8004100C** | Not supported | no instance: no read |
| 4 | `LENOVO_LIGHTING_METHOD.Get_Lighting_Current_Status(Lighting_ID=n)` | class-level invoke, n = 0..3 | **0x8004102F** | Invalid method Parameter(s) | invocation shape rejected, not a firmware verdict |
| 5 | `LENOVO_GAMEZONE_DATA.GetKeyboardLight` | instance `ACPI\PNP0C14\GMZN_0` | **0x8004100F** | Invalid object | per-method firmware rejection |
| 6 | `LENOVO_GAMEZONE_DATA.IsSupportLightingFeature` | same instance | **0x8004100F** | Invalid object | per-method firmware rejection |
| 7 | `LENOVO_GAMEZONE_DATA.GetKeyboardfeaturelist` | same instance | **0x8004100F** | Invalid object | per-method firmware rejection |
| 8 | `LENOVO_GAMEZONE_DATA.GetKeyboardLight` / `IsSupportLightingFeature` | class-level invoke | **0x8004102F** | Invalid method Parameter(s) | shape rejected |
| 9 | **control** `LENOVO_GAMEZONE_DATA.IsSupportGpuOC` | same instance | ok | `Data = 0` | provider + instance binding are healthy |
| 10 | `LENOVO_LIGHTING_EVENT` | instance enumeration | ok, count 0 | — | abstract event class, no instance |
| 11 | `LENOVO_LIGHTING_EVENT` | `ManagementEventWatcher.Start()` | ok | — | class is subscribable; no event was generated |
| 12 | `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT` | `ManagementEventWatcher.Start()` | ok | — | same |
| 13 | **control** `KBLC(0x1)` query | `\\.\EnergyDrv` IOCTL `0x83102144`, in u32 4 B, out u32 4 B | ok | `out = 0x00000007` | firmware answers: `KBGC=6`, group 3, states 0..3 |
| 14 | **control** `KBLC(0x32)` get, group 3 | same | ok | `out = 0x00000001` | `KBGS=0`, state 0 = off |
| 15 | **control** `KBLC(0x22)` get, group 2 | same | ok | `out = 0x00000000` | group mismatch → `0`, never "off" |

Row 4/8 is the important negative control: a class-level invoke of an
ACPI-WMI method is always `0x8004102F`, so a run that stops there says
nothing about the firmware. Only an **instance** invoke produces a firmware
verdict (rows 5–7, 9).

## 3. Declared schema (what the classes promise, and what they answer)

```text
LENOVO_LIGHTING_METHOD  provider=WmiProv dynamic=True guid={8c5b9127-ecd4-4657-980f-851019f99ca5}
  methods: Get_Lighting_Current_Status in=[Lighting_ID] out=[Current_Brightness_Level, Current_State_Type]
           Set_Lighting_Current_Status in=[Current_Brightness_Level, Current_State_Type, Lighting_ID] out=[]
  properties: Active, InstanceName

LENOVO_LIGHTING_DATA    provider=WmiProv dynamic=True guid={4dd5bd84-15a9-47e2-ad65-cc61a5c62fd0}
  methods: none
  properties: Active, Brightness_Level, Control_Interface, Default_Brightness_Level,
              Default_State, InstanceName, Lighting_Id, Lighting_Type, State_Type_Num

LENOVO_LIGHTING_EVENT   abstract=True guid={1e3391a1-2c89-464d-95d9-3028b72e7a33}
  properties: Active, InstanceName, Key_ID, SECURITY_DESCRIPTOR, TIME_CREATED

LENOVO_GAMEZONE_DATA    provider=WmiProv dynamic=True guid={887B54E3-DDDC-4B2C-8B88-68A26A8835D0}
  52 methods; lighting group: IsSupportLightingFeature, SetKeyboardLight, GetKeyboardLight,
                             GetKeyboardfeaturelist, SetLightControlOwner
  lighting signatures: in=[] out=[Data]  (u32 in the MOF)
  one instance: LENOVO_GAMEZONE_DATA.InstanceName="ACPI\\PNP0C14\\GMZN_0"
```

So the schema is installed on this machine in all three cases; what is
missing is the firmware behind it. `LENOVO_LIGHTING_METHOD` promises the
whole read contract (`Lighting_ID` in, brightness + state type out) and then
refuses to enumerate a single instance — the state in which the CLI's
`LENOVO_LIGHTING_METHOD` controller cannot be *constructed*, not merely
"returns an error later".

### 3.1 Complete lighting verb inventory (so nothing is left untried)

`LENOVO_GAMEZONE_DATA` declares 52 methods in total; exactly four are
lighting verbs:

```text
IsSupportLightingFeature   in=[]      out=[Data]   read   -> 0x8004100F INVALID_OBJECT
GetKeyboardLight           in=[]      out=[Data]   read   -> 0x8004100F INVALID_OBJECT
SetKeyboardLight           in=[Data]  out=[]       write  -> NOT INVOKED (no-write scope)
SetLightControlOwner       in=[Data]  out=[]       write  -> NOT INVOKED (no-write scope)
```

(`GetKeyboardfeaturelist` is not a lighting verb — it is the keyboard
*feature* list — but it was probed as well and answers `0x8004100F`.)

`LENOVO_LIGHTING_METHOD` declares exactly two, and both accepted their
declared parameter sets without being invoked:

```text
Get_Lighting_Current_Status  accepted-parameters = Lighting_ID                       read
Set_Lighting_Current_Status  accepted-parameters = Current_Brightness_Level,
                                                    Current_State_Type, Lighting_ID   write
```

The two `Set*` verbs were deliberately not invoked: the item's scope is
read-only, and the provider would be asked to change the backlight. That is
recorded as "declared and unreachable" rather than "absent", and it makes no
difference to the verdict — no read exists on the plane, so no write could be
verified either.

## 4. Root cause: the lighting ACPI-WMI mapper device is not on this machine

`LENOVO_LIGHTING_METHOD` / `LENOVO_LIGHTING_DATA` are ACPI-WMI mapper classes
(`provider=WmiProv`, `dynamic=True`): one instance per ACPI device that
publishes the matching method/`_WDG` GUID. The documented instance family is
`ACPI\PNP0C14\LLT_0` (`docs/04-peripherals.md` §2.1). This machine enumerates
these `ACPI\PNP0C14` instances, all `Status=OK`, `Problem=CM_PROB_NONE`:

```text
ASIS, ATAG, DSarDev, GMZN, ISET, LSK20, LSR, TestDev, WBAT      (9 instances)
ACPI\VPC2004\0                                                  (1 instance)
```

and the registry mirror agrees with no extra entry:

```text
HKLM\SYSTEM\CurrentControlSet\Enum\ACPI\PNP0C14  -> ASIS,ATAG,DSarDev,GMZN,ISET,LSK20,LSR,TestDev,WBAT
HKLM\SYSTEM\CurrentControlSet\Enum\ACPI\VPC2004  -> 0
ACPI device ids matching LIGHT|LLT|LED           -> (none)
```

There is a **GMZN** device (gamezone: it answers `IsSupportGpuOC`) and there
is **no** LLT/LIGHT/LED device. `Enum\ACPI` lists every ACPI device ID the
firmware declares, present or not, so the absence is a firmware declaration
absence for this boot, not a driver or permission problem. That is exactly
why rows 1–3 are `NOT_SUPPORTED` while the classes' schema still exists.

A grep of the wider WMI space finds no alternative lighting plane either:

```text
root\wmi   classes=1650  names matching LIGHT|BACKLIGHT:
           LENOVO_LIGHTING_EVENT, LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT,
           LENOVO_LIGHTING_DATA, LENOVO_LIGHTING_METHOD, SystemConfig_V2_FlightIds
root\cimv2 classes=1349  names matching LIGHT|BACKLIGHT: (none)
root\Lenovo             -> WBEM_E_INVALID_NAMESPACE ("无效命名空间")
```

`LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT` is the only lighting-named class
not in the earlier sweep; it is an event class, not a read, and it was not
observed to deliver anything (nothing was triggered).

## 5. The control that does answer

Same probe session, same machine, `CreateFileW("\\\\.\\EnergyDrv",
GENERIC_READ, OPEN_EXISTING)` + `DeviceIoControl(0x83102144, in u32, out 4)`:

```text
KBLC(0x1)  -> ok out=0x00000007 bytes_returned=4   group 3, states 0..3 (tristate + auto)
KBLC(0x32) -> ok out=0x00000001 bytes_returned=4   state 0 = off
KBLC(0x22) -> ok out=0x00000000 bytes_returned=4   group 2 != KBGC 6 -> zero
GENERIC_RW open + KBLC(0x1) -> ok out=0x00000007   (query only, no set issued)
```

The ACPI `VPC2004` device is present, its driver re-exposes these methods, and
the firmware answers. This is the single lighting read on 21VG, and it is the
channel already recovered on 2026-11-05.

## 6. Verdict and implementation boundary

- **No WMI lighting READ answers on this SKU** — not `LENOVO_LIGHTING_DATA`,
  not `LENOVO_LIGHTING_METHOD`, not the `LENOVO_GAMEZONE_DATA` lighting
  verbs. The rejections are firmware/model declarations (`NOT_SUPPORTED`,
  `INVALID_OBJECT`) on a healthy provider, proven by the sibling method that
  answers on the same instance.
- **Exactly one lighting READ answers**: ACPI `KBLC` (query + get) over
  `\\.\EnergyDrv` IOCTL `0x83102144`. It is already implemented —
  `crates/lctrl-hal-win/src/kbd_backlight.rs`
  (`WindowsKeyboardBacklight::layout`/`read_state`) — and already wired as the
  primary channel in the composition root
  (`vantage/src/main.rs:671-673`, `WindowsKeyboardSources::new(WindowsKeyboardBacklight::new(NativeIoctl), WindowsPeripheralController::new(Wmi))`).
  The capability probe records `kbd.backlight = Available` with the KBLC
  group/states in its reason (`crates/lctrl-hal-win/src/windows_hal.rs:296-311`).
- Therefore **nothing lighting-specific needs implementing from this retry**:
  the only answering read is live and the WMI planes have no read to build on.
  The WMI lighting controller stays in place as the fallback for RGB models
  that publish an LLT device; on 21VG it is unreachable by construction, and
  `WindowsKeyboardSources` only consults it when KBLC is absent.

## 7. Dependent defect found by this probe — repaired (Main-approved)

`crates/lctrl-hal-win/src/error.rs` declared
`WBEM_E_NOT_SUPPORTED = 0x8004_1001`, but `0x80041001` is `WBEM_E_FAILED`
(generic failure). `WBEM_E_NOT_SUPPORTED` is **`0x8004100C`** — the value this
probe measured together with the provider's own "Not supported" text, the
value `README.md:111`, `docs/01-hal-interfaces.md:298`,
`docs/03-thermal-performance.md:572` and `docs/04-peripherals.md:150` already
quote, and a different value from the `0x80041001` that
`docs/evidence/re-battery-health-20261105.md:29` records as "generic failure".

Effect before the repair: `map_wmi_hresult` did not recognize `0x8004100C`, so
every firmware-absent WMI channel that answers `不支持` surfaced as
`LctrlError::Io("... WMI HRESULT 0x8004100C")` — a transient I/O error —
instead of `Unsupported`. That defeated exactly the consumers keyed on
absence: `WindowsKeyboardSources::channel_absent`
(`crates/lctrl-hal-win/src/kbd_backlight.rs`) refuses to fall back on `Io`,
`active_instance` (`crates/lctrl-hal-win/src/wmi_contract.rs`) reports the
absence as a raw I/O failure, and the capability registry caches a channel as
unavailable only for `Unsupported`.

Channels that were misreported because of it (each answers `0x8004100C` on
21VG; call sites verified in source):

| Channel / feature | Code path | Answering plane |
| --- | --- | --- |
| `kbd.backlight` WMI fallback | `crates/lctrl-hal-win/src/peripherals.rs:36,58,79` | `LENOVO_LIGHTING_DATA`, `LENOVO_LIGHTING_METHOD` |
| `kbd.backlight` capability probe | `crates/lctrl-hal-win/src/windows_hal.rs:311` | `active_instance(LENOVO_LIGHTING_DATA)` |
| `perf.fan.curve` | `crates/lctrl-hal-win/src/gamezone.rs:173` (`Fan_Get_Table`) | `LENOVO_FAN_METHOD` |
| GPU-OC status | `crates/lctrl-hal-win/src/gpu_oc.rs:18`, `windows_hal.rs:737` | `LENOVO_GAMEZONE_GPU_OC_DATA` |
| GPU-OC / OC report surfaces | capability and report text only (no live query in Rust today) | `LENOVO_GPU_OVERCLOCKING_DATA`, `LENOVO_GPU_METHOD`, `LENOVO_CPU_OVERCLOCKING_DATA`, `LENOVO_MEMORY_OC_DATA`, `LENOVO_DISCRETE_DATA` (`docs/evidence/re-gpu-oc-20261105.md:23-27`) |
| panel colour/overdrive | evidence only — `panel_features.rs` uses no WMI class today | `LENOVO_PANEL_METHOD` (`docs/evidence/wmi-capability-exhaustion-20261104.md` §2) |

Two different consequences, both fixed by the same constant: for the
`kbd.backlight` fallback/capability and `perf.fan.curve` the *error class*
changed (`Io` → `Unsupported`, which is what the fallback and capability cache
key on); for `perf.gpu` the capability boolean already ignored the error and
`gpu_oc_status` keeps the verdict as text, so only that text changes from
"WMI HRESULT 0x8004100C" to an unsupported verdict.

Applied repair (approved by Main, this work item):

1. `crates/lctrl-hal-win/src/error.rs`: `WBEM_E_NOT_SUPPORTED` corrected to
   `0x8004_100C` inside the `Unsupported` arm, with a comment pinning
   `0x80041001` as `WBEM_E_FAILED` that must stay outside it.
   `WBEM_E_INVALID_METHOD_PARAMETERS = 0x8004_102F` added to the same arm —
   the live answer of a class-level ACPI-WMI invoke (rows 4 and 8).
2. `crates/lctrl-hal-win/tests/error_mapping.rs`:
   `wmi_not_supported_marks_feature_unsupported` now asserts
   `0x8004_100C -> unsupported` (it previously encoded the wrong constant),
   plus two regression cases: `0x8004_1001 -> io` (generic failure is not
   absence) and `0x8004_102F -> unsupported`.

Residual trade-off of folding `0x8004102F` in: a caller's genuinely malformed
argument also produces it. It is accepted because the HAL only ever builds
method arguments through typed `WmiValue` inserts, and the only producer
observed on the target is the class-level invocation shape; a wrong argument
would surface as `Unsupported` rather than `Io`, which is still a refusal and
never a silent success.

No other place in the workspace hardcodes `0x8004_1001` or `0x80041001` with a
"not supported" meaning: the only two occurrences were `error.rs:17` and the
test (both fixed), and `docs/02-power-battery.md:389,609` correctly label it
"generic failure".

## 8. Not observed / parent checks

Nothing below was run here (this seat does not execute the built CLI):

- `vantage kbd backlight` read on the current build should print
  `level=0, max=3, effect=static` (state 0 was the live KBLC answer);
  capability output should show `kbd.backlight Available` with the
  "ACPI VPC KBLC keyboard group 3 exposes states 0..=3 (auto included)"
  reason.
- `vantage kbd backlight high --dry-run` must issue **only** query + get
  (no `0x83102144` set argument), then `high` → readback, then `off`.
- Negative control for the WMI fallback: with the KBLC device unavailable
  (cannot be produced on this machine without stopping the VPC driver, so
  this is a unit-test concern, not a target test), the fallback must fail
  `Unsupported` and the reported `kbd.backlight` reason must name KBLC.
- Negative control for the HRESULT repair: a `LENOVO_FAN_METHOD.Fan_Get_Table`
  call must now report `unsupported`, not `io`; a `0x80041001` reply must
  remain an I/O error (`wmi_generic_failure_is_not_absence`); and the
  capability text for `perf.fan.curve` / `kbd.backlight` must change from
  "WMI HRESULT 0x8004100C" to an `unsupported` verdict.

## 9. Scope statement

Read-only throughout: instance/class/schema queries; three lighting read verbs
invoked on the live GAMEZONE instance (`GetKeyboardLight`,
`IsSupportLightingFeature`, and the non-lighting `GetKeyboardfeaturelist` as a
comparison), all answered `0x8004100F`; two class-level invocation shapes
(`0x8004102F`, the fixed rejection of an instance method invoked on the class);
`IsSupportGpuOC` as the positive control; the `Get_Lighting_Current_Status`
parameter set requested without invoking it; two event-class subscriptions
that were started and stopped without waiting; and one device open per KBLC
call with query/get arguments only. No `Set*` WMI method, no
`Set_Lighting_Current_Status`, no `SetKeyboardLight`, no `SetLightControlOwner`,
no registry write, no service change, no BIOS staging touch.

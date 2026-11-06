# Fan-mode write — contract, gating reconciliation, and why no write is reachable — 2026-11-05

Target: `tb14p-ezra` (ThinkBook 21VG). Sources: live WMI/registry probes and
`LNVDispatcherService.exe` (the `LenovoProcessManagement` service).

Binary analysed (copied from the driver store for static work):

```
C:\Windows\System32\DriverStore\FileRepository\lnvprocessmanagement.inf_amd64_863cdf71782696c8\LNVDispatcherService.exe
bytes   1354072
sha256  d536f3c0c23ddf92baef5ac57ba3af66a792e3bbe858f933c731195948ee88d8
service LenovoProcessManagement, Running (pid 6160 at probe time)
```

Scratch evidence on the target (not committed): `fanrecon.txt`,
`fanwmi2.txt`..`fanwmi6.txt`, `fanvbs.txt`, `fanwrite3.txt`, `fandomain.txt`.

## 1. The fan surface that exists, and what it answers

`LENOVO_GAMEZONE_DATA` is present with an active instance
`ACPI\PNP0C14\GMZN_0` and **52 methods**. Signatures were enumerated through
`SWbemObject.Methods_` (PowerShell `Get-CimClass` reports zero methods for this
class — a CIM-cmdlet path artefact, not the provider's real surface):

| method | in | out |
|---|---|---|
| `GetSmartFanMode` | – | `Data: UInt32` |
| `SetSmartFanMode` | `Data: UInt32` | – |
| `GetSmartFanSetting` | – | `Data: UInt32` |
| `IsSupportSmartFan` | – | `Data: UInt32` |
| `IsSupportFanCooling` / `SetFanCooling` | – / `Data: UInt32` | `Data: UInt32` / – |
| `GetFanCoolingStatus` | – | `Data: UInt32` |
| `GetThermalMode` / `SetLightControlOwner` / `SetDDSControlOwner` | – / `Data` / `Data` | `Data: UInt32` / – / – |

All parameter/return `Data` fields are CIM type 19 (`UInt32`). There is **no**
`SetFanTale`, `Fan_Set_Table` or curve method in this class.

Live results (`fan.vbs`, raw SWbem HRESULTs, 2026-11-05T16:25:46):

```
GetVersion             ERR 0x8004100F Invalid object
IsSupportSmartFan      ERR 0x8004100F Invalid object
GetSmartFanMode        ERR 0x8004100F Invalid object
GetSmartFanSetting     ERR 0x8004100F Invalid object
GetFanCoolingStatus    ERR 0x8004100F Invalid object
IsSupportFanCooling    ERR 0x8004100F Invalid object
GetThermalMode         ERR 0x8004100F Invalid object
IsSupportGpuOC         OK  Data=0
SetSmartFanMode[i4]    OK  (no out params)
SetSmartFanMode[bstr]  OK  (no out params)
ITS_FanMode            4   (unchanged)
```

`0x8004100F` is `WBEM_E_INVALID_OBJECT`: the provider is alive and answers
`IsSupportGpuOC`, but the whole fan getter/probe group is absent in this
firmware. So the mode is **not readable**, which also means the CLI cannot
verify any fan write.

## 2. `SetSmartFanMode` acknowledges, but does not validate or apply

Repeated writes were issued with a fresh SWbem connection per process, and the
value, the VARIANT type and the inter-call delay were varied. The acceptance
pattern does not depend on any of them:

| probe | first call in process | later calls in same process |
|---|---|---|
| `fanwmi2` 16:22 | `Data=4` as VT_BSTR → OK | `Data=4` as VT_I4 → `0x80004005` |
| `fanwmi3` 16:23 | `Data=4` VT_BSTR → OK | VT_I4 / VT_UI2 / VT_UI8 → `0x80004005` |
| `fanwrite3` 17:10 | `Data=3` VT_I4 → OK | `Data=4` after 60 s → `0x80004005` |
| `fandomain` 18:55 | `Data=0` VT_I4 → ACCEPTED | `1, 2, 3, 5, 255, 0` → `0x80004005` each |

`0x80004005` = `E_UNEXPECTED`, returned by the provider, not by the transport.

Interpretation (the only hypothesis consistent with all four runs): the
provider acknowledges **one** `SetSmartFanMode` per caller process and rejects
every later one. There is no value-dependent acceptance anywhere in
`0 … 5, 255`, so **no fan-mode domain is observable through this provider**,
and the acknowledgement cannot be distinguished from a no-op.

No state changed across those writes:

* `ITS_FanMode` stayed `4` throughout (polled every 10 s for 60 s in
  `fanwrite3`).
* ACPI thermal zones stayed flat (`34.1 °C`, idle).
* `Win32_Fan` exists but exposes no `DesiredSpeed`/`VariableSpeed`, so there is
  no OS-level fan-speed observation either.

Consequence: a CLI that treats `SetSmartFanMode`'s `S_OK` as a successful
fan-mode write is reporting a success it cannot observe. That is the fake
success this change removes.

## 3. `ITS_FanMode` is a dispatcher write-only projection, not a control

Registry: `HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`

Live (2026-11-05T14:46): `ITS_FanMode=4`, `ITS_CurrentSetting=3`,
`ITS_CurrentSettingV=3`, `ITS_AutomaticModeSetting=7`, `Policy_ModernITS=1`.

Static (dispatcher image):

* The wide string `ITS_FanMode` exists **once** in the whole image
  (file offset `0xf24c0`, RVA `0xf30c0`).
* Its only code reference is the DWORD writer at `0x14000be90`
  (`lea r8, [rip+…]` → `0x1400f30c0`, `lea rdx, [rip+…]` → the
  `SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`
  path), which tail-calls `0x14000b880` — a `RegOpenKeyExW` + `RegSetValueExW`
  helper ("CreateDWORD in PowerSlider").
* The writer is called from `0x14000abb0` (`call 0x14000be90` at `0x14000ac7b`),
  a command-handler-shaped function with no static callers (reached through the
  service's dispatch table / named pipe, `CDispatchCommand`).

Nothing in the service reads `ITS_FanMode`; there is no second reference that
could be a read path. A client write to that value therefore cannot reach the
firmware, and the dispatcher would overwrite it on its next apply. It also
cannot be a mirror of `GetSmartFanMode`, because that call is unsupported on
this firmware (§1): the value's relationship to any firmware fan state is
**unestablished**, which is why `FanMode::from_raw(4)` renders as
`unknown (4)` and not as a named mode.

## 4. `SetFanTale` is dead in this build

`SetFanTale` appears only in three log-format strings:

```
0x1400fe720  MLScenarioWorkerThread::[SetFanTale]= %hX  Success=%d ,CurrentFun= %hX ,Valid Flag= %hX
0x1400fe780  MLScenarioWorkerThread::[SetFanTale_TryAgain]= %hX  Success=%d ,CurrentFun= %hX ,Valid Flag= %hX
0x1400fe7f0  MLScenarioWorkerThread::Dispatcher 3.x _return= %d _PITSMode = %d _NewITSMode = %d,PL1= %d , PL2= %d
```

A full RIP-relative sweep of `.text` finds **zero** references into
`0x1400fe000–0x1400ff000`, and the class exposes no `SetFanTale` method.
The verb is legacy text carried in the image; the dispatcher's live fan call is
`LENOVO_GAMEZONE_DATA.SetSmartFanMode`.

Recovered dispatcher call site (`0x140019080`), for the record:

```
0x14001917e  lea rcx,[rip+…] -> wide "SetSmartFanMode"   (0x1400f7e38)
0x140019190  lea rax,[rip+…] -> wide "Data"              (0x1400f7e58)
0x140019361  mov word ptr [rsp+0xb0], 8                  ; VARIANT vt = VT_BSTR
0x140019395  call 0x140019750                            ; format the integer into the BSTR buffer
0x14001939a  … call [rax+0x28]                           ; IWbemClassObject::Put("Data", …)
```

i.e. the vendor client sends the mode as a **decimal string VARIANT**, and the
class/namespace/instance literals it uses are
`LENOVO_GAMEZONE_DATA` (`0x1400f7e08`), `root\wmi` (`0x1400f7df0`) and
`LENOVO_GAMEZONE_DATA.InstanceName='ACPI\PNP0C14\GMZN_0'` (`0x1400f7d80`).
This matches the value-independent acceptance in §2: the provider does not
convert the string into a validated fan mode.

## 5. Why `perf.fan.*` reported `unsupported` — the actual gating chain

1. `windows_hal::capabilities` recorded `perf.fan.mode` from
   `GameZone::smart_fan_mode()` (`GetSmartFanMode`) alone. On 21VG that call
   returns `WBEM_E_INVALID_OBJECT`, so the feature was recorded
   `Unavailable` (`vantage capability` → `unsupported`, observed in
   `capdump5.txt` for `perf.fan.auto|manual|fullspeed|smart`).
2. Those four commands bind `FeatureBinding::Capability("perf.fan.mode")`, and
   `capability_presence` maps `Unavailable → FeaturePresence::Unsupported`.
3. `thermal::set_fan_mode` wrote through `GameZone::set_smart_fan_mode` and only
   then verified with `poll_readback(…, fan_mode())`, where `fan_mode()` falls
   back to `ITS_FanMode`. Because that projection is not updated by our write,
   the readback always disagreed: the WMI write was issued, the command failed
   with `VerifyMismatch`, and `--auto-restore` then wrote again.

So the gate was attached to the wrong leg (read), and the write it was gating
was both unverifiable and unvalidated.

## 6. Decision

* No fan-mode write is reachable *and verifiable* on this SKU:
  getter absent, setter a one-shot acknowledgement, registry projection
  write-only, `SetFanTale` dead, `LENOVO_FAN_METHOD` rejects (`不支持`).
* Therefore `set_fan_mode` pre-flights the firmware readback
  (`GetSmartFanMode`) and refuses to write when it is unavailable, reporting
  the firmware error. Nothing is written when the result could not be checked.
* `perf.fan.mode` reports `Available` only from a live firmware read,
  `Limited` when only the dispatcher projection is readable, `Unavailable`
  when neither is.
* New feature `perf.fan.mode.write` records the write channel and the four
  write commands bind to it, so `vantage capability` says *why* they are
  unavailable instead of inheriting a read probe's failure.

## 7. Limits of this evidence

* The vendor dispatcher's own WMI fan calls could not be observed directly
  (its log surface is not readable here); the claim that they fail rests on the
  provider behaviour in §1–§2, which is caller-independent at the WMI layer.
* A SYSTEM-context re-run of the getter was attempted through a scheduled task
  (`fansys2.txt`, `fansys3.txt`): the task ran (Last Result 0) but produced no
  output file, so the SYSTEM datapoint is **inconclusive**, not negative. The
  provider returns `WBEM_E_INVALID_OBJECT` for a local administrator, and WMI
  method ACLs are not applicable to this ACPI provider (ACPI MOF classes carry
  no method-level ACLs).
* The exact firmware semantics of `SetSmartFanMode` Data (and hence whether any
  value would "work" on another SKU) remain unresolved; this document does not
  claim a mapping.
* The one-shot acceptance behaviour is inferred from four independent runs with
  different values, types and delays; it is a black-box observation, not a
  disassembled fact.

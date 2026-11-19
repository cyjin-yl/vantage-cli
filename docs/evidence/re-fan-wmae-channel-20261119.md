# Fan tach read + duty write — the REAL channel: `LENOVO_OTHER_METHOD` → WMAE → ECCC 0x62/0x64 — 2026-11-19

Target: `tb14p-ezra` (ThinkBook 21VG). Source: DSDT decompile (`iasl -d`,
DSDT-CB01-0_261655.aml v02 LENOVO CB-01) + live WMI invocation on the running
machine. Supersedes `re-fan-rpm-20261106.md` ("no tach readable") and
`re-fan-write-20261105.md` ("no write reachable").

## 1. Why earlier probes missed it

`LENOVO_GAMEZONE_DATA` exposes 52 MOF methods, but every one of them binds to
ACPI method `WMAA` (WmiMethodId 4–68). In the DSDT `WMAA` is a **stub**:

```
Method (WMAA, 3, NotSerialized)
{
    Local0 = Arg1
    ADBG (Concatenate ("WMAA Method ID=", ...))
    If ((Arg1 == 0x04)) { Return (Zero) }
}
```

Arg1==0x04 is `IsSupportGpuOC` — the only GAMEZONE method that ever worked.
Every other call falls through → `WBEM_E_INVALID_OBJECT` on getters, and
`SetSmartFanMode` (ID 44) "accepted once per process" because the WMI provider
caches the stub's void return. This is the full ACPI-level explanation for the
2026-11-05 behaviour table.

`LENOVO_FAN_METHOD.Fan_Get_Table` / `Fan_Set_Table` bind `WQC2`/`WQC3`-adjacent
query paths that return "Not supported" — they never write fan duty.

## 2. The real device: `_SB.GZFD` (PNP0C14, UID `GMZN`)

`_WDG` registers 7 blocks; the method entries are:

| ObjectId | flags | handler | binding |
|---|---|---|---|
| `AA` | method | `WMAA` (stub) | `LENOVO_GAMEZONE_DATA` (52 dead methods) |
| `AE` | method | `WMAE` (real) | **`LENOVO_OTHER_METHOD`** |
| `A9` | query×11 | `WQA9` | system cap table |
| `C2` | query | `WQC2` | RPM/trip table (returns 5100/4500/3000 RPM under OD08) |
| `C3` | query | `WQC3` | RPM range table by PJID |
| `AD` | query×22 | `WQAD` | display refresh info |
| `DD` | event | — | PNP0C14 notification GUID |

## 3. `WMAE` dispatcher contract (verified live)

`root\wmi:LENOVO_OTHER_METHOD`, instance `ACPI\PNP0C14\GMZN_0`:

| WMI method | WMAE ID | in | out |
|---|---|---|---|
| `GetFeatureValue` | 0x11 | `IDs:UInt32` (→ SIDS) | `value:UInt32` |
| `SetFeatureValue` | 0x12 | `IDs:UInt32`, `value:UInt32` (→ SVAL) | — |
| `GetDataByCommand` | 0x13 | `Command:UInt32` (→ IAG2), `IDs:UInt32` | `Data[]`,`DataSize` |
| `GetDataByPackage` | 0x63 | `Input:UInt8[]` mailbox | `Data[]`,`DataSize` |

Handshake: `GetFeatureValue(0x00230000)` → `1` (verified).

### SIDS map (from DSDT; ✓ = live-verified)

| SIDS | r/w | ACPI effect | live result |
|---|---|---|---|
| 0x00230000 | r | handshake | ✓ =1 |
| 0x01010000 | r/w | `PL2V`; write→`TCPU.CSTP = v*8` | r=0 |
| 0x01020000 | r/w | `PL1V`; write→`TCPU.CLTP = v*8` | r=0 |
| **0x04030001** | **r/w** | **r: `ECCC(0x64)→ECRT` (RPM); w: `ECCC(0x62, v/100 clamped 0xFF)`** | **✓ RPM + duty** |
| 0x04030002/3/4 | r | constants 0x0BB8 (=3000) | r=3000 |
| 0x08010001 | r | `BTFW` | r=2 |
| 0x0A010000 | r | `MSGV` | — |
| 0x0A020000 | r | `QCRT*0xFA0` temp conv | — |
| 0x001C0001/2 | w | `ATS4/ATS5` EC fields | — |
| 0x0A010000 | w | `MSGV` + `PMBM(0x22,…)` LED? | — |
| 0x08010002 + IAG2{0x30,0x50,0x60,0x62,0x3E} | r via GetDataByCommand | `OF30/OF3E/OF40/OF60/OF3C` mailbox buffer reads | — |

## 4. Live write/read verification (2026-11-19)

Invoke path: `ManagementObject.InvokeMethod` with `GetMethodParameters` —
`Invoke-WmiMethod -ArgumentList` and `Get-CimClass` both fail ("Invalid
parameter"/zero methods); SWbemWorks only.

```
baseline RPM            = 1838
SetFeatureValue(0x04030001, 6000)   # 60% duty
t+0..2750ms: 1871 → 1990 → 2186 → 2356 → 2585 → 2796 → 3147 → 3337 → 3494 → 3685 → 3871 → 4030 → 4244
SetFeatureValue(0x04030001, 3000)   # 30% duty
t+0..2750ms: 4244 → 4418 → 4409 → 4312 → 4154 → 3978 → 3816 → 3636 → 3494 → 3302 → 3115 → 2945
```

Fan tracks duty within ~250ms. The override **latches**: after 20s idle ITS
did not retake; there is no release-to-auto SIDS.

### duty=0 boundary

```
SetFeatureValue(0x04030001, 0)
RPM: 2998 → 2941 → 2642 → 2298 → 1911 → 1799 → 1969 → 1875 → (15s) 1930
```

Duty 0 does **not** stop the fan — the EC enforces an ~1800 RPM floor. The
override cannot be weaponised into fan-off; thermal floor is firmware-held.

### Restore

`SetFeatureValue(0x04030001, 1800)` → RPM settled ~1780-1854. A written duty
holds until overwritten or reboot; the EC floor (~1800 RPM) applies at the low
end, so the safest "auto-like" resting write is a low duty (EC floor decides).

## 5. What this unlocks / changes

1. **`perf.fan.rpm` read**: `GetFeatureValue(0x04030001)` — real tach.
   `re-fan-rpm-20261106.md` is superseded on this one point (its conclusion
   held for every OTHER channel; this one was never probed).
2. **`perf.fan.duty` write**: `SetFeatureValue(0x04030001, duty_hundredths)`.
   Direct EC duty; latched until overwritten; EC floor ~1800 RPM at duty≤low.
   Requires caller-owned periodic refresh or explicit "release" semantics —
   there is no firmware auto-revert.
3. **PL1/PL2 write**: `SetFeatureValue(0x01010000/0x01020000, watts)` →
   `TCPU.CSTP`/`CLTP` (value×8). Independent of DYTC/ITS path.
4. **RPM tables**: `WQC2`/`WQC3` classes exist as data objects
   (`LENOVO_FAN_MAX_SPEED_DATA`?); the DSDT returns per-mode RPM bounds.
5. `SetSmartFanMode`/`GetSmartFanMode`/`Fan_Set_Table` remain dead (WMAA stub)
   — do not retry them; the capability detail texts should point here.

## 6. CLI wiring notes

- Invocation MUST use `ManagementObjectSearcher` + `GetMethodParameters` +
  `InvokeMethod(name, inParams, options)`; `Invoke-WmiMethod`/`Invoke-CimMethod`
  drop/renumber the undeclared params.
- `value` for duty is **hundredths of percent** (6000=60%), firmware
  divides by 0x64 and clamps at 0xFF.
- Missing release: recommend daemon-style periodic duty writes or a
  documented "vantage owns fan until process exit" contract; on exit write a
  low duty (EC floor engages) — or leave last duty (it is latched safely).

## 7. State gate discovered 2026-11-19 (second session)

After the §4 write series the machine was left with duty latched near the EC
floor. In a later session **all** `GetFeatureValue(0x04030001)` reads returned
`0` and every `SetFeatureValue(0x04030001, …)` write was ignored — including
an 80% write held for 8 s and a sustained 16-thread CPU load.

Working model: `ECCC(0x62)`/`ECRT` respond **only while the EC holds the
manual-override armed**. The earlier `duty=0` write did not hit a floor —
it *released* the override and handed the fan back to ITS/DPTF auto. Under
auto control at ~35 °C the fan is legitimately stopped, so `ECRT` reads 0
and `ECCC(0x62)` is ignored. This matches the tach never changing under the
forced CPU load (ITS kept the fan off through the whole window).

**Re-arming the override is not exposed through `LENOVO_OTHER_METHOD` SIDS.**
The arming path almost certainly lives in the ITS/DYTC layer: DYTC `SET MMC`
(`ICFunc=0x0B`, `VMMC=1`) sets "manual fan mode" in the DSDT, and the EC
command `ECCC(0x62)` is then the duty register for that mode. No WMAE SIDS
writes `VMMC`; `ITS_FanMode`/`ITS_CurrentSetting` are the dispatcher
projection the vendor stack uses to reach it, but those registry writes
require elevation this session did not have.

**Verified anyway**: the write channel is real — RPM tracked 60%→30%→0%
within ~250 ms per step (§4). What needs solving before `set_fan_duty` is
production-useful is the ITS manual-mode arming step, not the duty path.

Until the arming mechanism is recovered, `set_fan_duty` is effective only in
the narrow window where the EC still holds a previously-written override
(same power-on session, before the first `duty=0`/auto release).

# Night-charge write verification — 21VG ThinkBook 14 G8+ IPH `TB14P-EZRA` — 2026-11-06

Scope: one bounded, reversible night-charge cycle against the live device, to
prove the recovered `\\.\EnergyDrv` contract is a real write channel rather than
a request that returns success and changes nothing.

| | |
| --- | --- |
| Target | `TB14P-EZRA` (Lenovo 21VG ThinkBook 14 G8+ IPH), user `tb14p-ezra\ezra` |
| OS | Windows 10 Pro for Workstations, display version 26H2, build `26300.9711` |
| Time zone | China Standard Time (`UTC+08:00`) |
| Cycle window | `2026-11-06T02:34:19+08:00` … `2026-11-06T04:00:08+08:00` |
| Binary under test | `C:\Users\ezra\vantage-target\debug\vantage.exe`, 10 413 056 bytes, mtime `2026-11-05T14:23:35+08:00`, SHA-256 `645B8B7BE73672290F9FE2D0FBC702DB39566E3A6D1666B5159F09270DA3E2DA` |
| Vendor DLL read | `C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\1.0.13.114\PowerBattery.dll`, SHA-256 `DF7A014A15630DBFA36603014E15B8B729EC500840B6F45CFB853BB957C10E91` |
| Authorization | parent item NIGHT-CHARGE-VERIFY: exactly one bounded reversible cycle (read → set opposite → independent readback → restore → verify restoration) |

Final device state equals the initial state (night charge **On**). No write was
issued outside the cycle below, and no test/build/lint was run.

## 1. Contract (recovered from the vendor binary)

Every claim in this section is read off the disassembly of the DLL above
(`/tmp/vantage-re/pb.asm`, produced from that exact SHA-256); it is not inferred
from the Rust source.

| Element | Value | Binary evidence |
| --- | --- | --- |
| Device | `\\.\EnergyDrv`, `GENERIC_READ\|GENERIC_WRITE` (`0xC0000000`), share `3`, `OPEN_EXISTING` (`3`), `FILE_ATTRIBUTE_NORMAL` (`0x80`) | wide string at `.rdata` RVA `0x17f18`; `CreateFileW` call at RVA `0xac50` |
| IOCTL | `0x83102150` | `mov edx,0x83102150` at RVAs `0xa61f`, `0xa7e9`, `0xa9b0` |
| Buffers | input = one `DWORD` selector (4 bytes), output = one `DWORD` value (4 bytes) | `DeviceIoControl` wrapper at RVA `0xad80` (`inSize=r9d=4`, `outSize=[rsp+0x28]=4`) |
| Support probe (selector `0x13`) | supported iff `(raw & 1) != 0 && ((raw >> 4) & 1) != 0` | `DoesSupportNightChargeMode` path, RVA `0xa5c0` |
| State read (selector `0x11`) | On iff `(raw >> 4) != 0`, i.e. bit 4 | `GetNightChargeMode`, RVA `0xa770` |
| State write | `SetNightChargeMode(1)` → selector `0x80000012`; `SetNightChargeMode(2)` → selector `0x12` | RVA `0xa930`: `mov [rsp+0x48],0x80000012` / `mov [rsp+0x48],0x12` |
| Post-write verification | the setter calls `GetNightChargeMode` and retries up to 10 × 50 ms until the readback equals the request | RVA `0xaa30`…`0xaa4b` (`Sleep(0x32)`, `cmp ebx,0xa`) |
| Value domain | `NotSupported=0`, `On=1`, `Off=2` (`BatteryNightChargeModeType`) | `BatteryManagementContract.dll` IL for the enum |

Exported names confirmed in the DLL export table:
`?GetNightChargeMode@CChargingMode@PowerBattery@@SAHXZ`,
`?SetNightChargeMode@CChargingMode@PowerBattery@@SAHH@Z`,
`?DoesSupportNightChargeMode@CChargingMode@PowerBattery@@SAHXZ`.

The write side and the read side are different encodings and must not be
conflated: the write input is a *command* word (`0x12` = set off,
`0x80000012` = set on), while the read answer is a *status bitmask* returned
from a different selector (`0x11`), where bit 4 is the state. Both write words
have bit 4 set, so neither can be interpreted as state — state is only ever the
`0x11` response, and that is the only thing the readback poll compares.

## 2. Implementation under test

| Site | Content |
| --- | --- |
| `crates/lctrl-hal-win/src/codec.rs:17` | `IOCTL_NIGHT_CHARGE = 0x8310_2150` |
| `crates/lctrl-hal-win/src/codec.rs:118-134` | `NightChargeSelector`: `SUPPORT_PROBE=0x13`, `STATUS=0x11`, `SET_ON=0x8000_0012`, `SET_OFF=0x12`, `encode()` = LE `u32` |
| `crates/lctrl-hal-win/src/native_ioctl.rs:73-79` | request validation: `0x13`/`0x11` open read-only, `0x12` open read+write, everything else rejected |
| `crates/lctrl-hal-win/src/ioctl_contract.rs:98-124` | `night_charge_probe`, `night_charge_status`, `write_night_charge(on)` |
| `crates/lctrl-hal-win/src/p0.rs:171-205` | read path; commit path writes then `poll_readback(…, 10, 50 ms)`, and on mismatch restores the previous state before returning the error |
| `crates/lctrl-hal-win/src/windows_hal.rs:372-382` | availability recorded as `Available` only when `EnergyDriver::night_charge_probe()` succeeds |
| `crates/lctrl-hal/src/command_capability.rs:339-349` | `BATTERY_NIGHT_CHARGE`: `Readback::Required`, `Rollback::PreviousState`, `DispatchSupport::Implemented`, requires `Dependency::EnergyDriver` |
| `crates/vantage-cli/src/lib.rs:2297`, `:3151` | `execute_night_charge` (honours `--dry-run`, optional `--auto-restore`), `restore_night_charge` |

## 3. Method — three independent observers

The cycle was judged by three readers that do not share code:

1. **A**: a standalone PowerShell + C# `CreateFileW`/`DeviceIoControl` client
   written from §1, opening a **fresh handle for every single query**
   (`C:\Users\ezra\nc-probe.ps1`, SHA-256
   `9F17E01878BE16E621F906E70A19E9FC63CDDCD99DA7AEA9608970E6A63D6B25`,
   identical hash on the host and on the target).
2. **B**: the vendor's own decoder — `PowerBattery.dll` loaded and
   `CChargingMode::GetNightChargeMode` / `DoesSupportNightChargeMode` called
   directly (`C:\Users\ezra\nc-vendorread.ps1`, SHA-256
   `036D7178410C31EAC8A1C8E78F17BF5361061601DD3C1A42301395322655F712`).
3. **C**: the CLI's own read path, `vantage --json battery night-charge <toggle>
   --dry-run`, which resolves the previous state through the HAL without writing.

The write itself was performed through the CLI commit path (C → HAL →
`ioctl_contract` → `\\.\EnergyDrv` → EC → readback poll), so the executed writer
is the shipped code, not observer A.

## 4. Cycle and results

| # | Step | Time (UTC+08:00) | Observation |
| --- | --- | --- | --- |
| 1 | A: read baseline | `02:34:19.685` | `probe13=OK:4 probeValue=0x00000011`, `status11=OK:4 statusValue=0x00000011`, `supported=True`, `enabled=True` |
| 2 | C: `night-charge off --dry-run` | `~02:52` | text `night charge: false`; JSON `{"mode":"dry_run","previous":true,"requested":true}` |
| 3 | B: vendor read | `02:52:34.278` | `GetNightChargeMode = 1 (On)`, `DoesSupportNightChargeMode = 1` |
| 4 | **write 1** — `vantage --json battery night-charge off --yes` | `~02:53:2x` | `{"actual":false,"mode":"commit","previous":true,"requested":false}`, exit `0` |
| 5 | A: read after write 1 | `02:53:27.453` | `probeValue=0x00000011` (unchanged), `statusValue=0x00000001`, `supported=True`, `enabled=False` |
| 6 | B: vendor read | `02:53:28.616` | `GetNightChargeMode = 2 (Off)`, `DoesSupportNightChargeMode = 1` |
| 7 | C: `night-charge on --dry-run` | `02:53:29` | `{"mode":"dry_run","previous":false,"requested":true}` |
| 8 | **restore** — `vantage --json battery night-charge on --yes` | `~02:58:5x` | `{"actual":true,"mode":"commit","previous":false,"requested":true}`, exit `0` |
| 9 | A: read after restore | `02:58:55.287` | `probeValue=0x00000011`, `statusValue=0x00000011`, `supported=True`, `enabled=True` |
| 10 | B: vendor read | `02:58:56.379` | `GetNightChargeMode = 1 (On)`, `DoesSupportNightChargeMode = 1` |
| 11 | C: `night-charge off --dry-run` | `02:58:57` | `{"mode":"dry_run","previous":true,"requested":false}` |
| 12 | A: re-read one hour later | `04:00:08.182` | `probeValue=0x00000011`, `statusValue=0x00000011`, `supported=True`, `enabled=True` — no drift, no vendor add-in re-asserting a different value in the interval |

Result: **no mismatch at any step.** The state word moved
`0x00000011 → 0x00000001 → 0x00000011` — exactly bit 4, twice, and nothing
else; the support word stayed `0x00000011` throughout, so the write changed the
setting without perturbing the capability bitmap. All three observers agreed at
each checkpoint, and the final state is identical to the initial state.

Live capability row (C, `--json capability`, `03:04:11`):

```json
{"axes":{"layer":"runtime","read":true,"readback":"required","rollback":"previous_state","write":true},
 "command":"battery.night-charge",
 "dependencies":[{"dependency":"energy_driver","usage":"required"}],
 "detail":null,"feature_ids":["battery.night-charge"],"hardware":"unknown",
 "layer":"runtime","service_roles":["battery"],"status":"available"}
```

`status=available` with `detail=null` means the probe-backed availability gate
opened on this SKU; the row is no longer the static
`DispatchSupport::Unsupported`.

## 5. Safety gate actually fired (and why it was overridden)

The first commit attempt was **refused without writing**:

```
exit 2
{"error":{"kind":"invalid_argument","message":"invalid argument: stop
 LenovoMagiCenterDetector.exe, LenovoPcManagerService.exe,
 LenovoVantage-(GenericMessagingAddin).exe, … LenovoVantageService.exe
 before writing, or re-run with --yes to accept concurrent-control risk", …}}
```

Vantage add-ins and `LenovoPcManagerService.exe` are running on this machine, so
the vendor-conflict gate (`confirm_risky_change`, `crates/vantage-cli/src/lib.rs`)
stopped the write. The two cycle writes were then run with `--yes`, which is the
flag the gate itself names and the item's explicit write authorization covers.
The gate's refusal is evidence that the guard is real and that the earlier "the
command did not write" conclusion is not an assumption.

The gate was **not** bypassed for anything beyond the authorized cycle; no Vantage
process was stopped, restarted or reconfigured.

## 6. Controls, and what was not verified

Observed controls:

- **Cross-channel isolation**: `battery status` reported `charge_mode = "rapid"`
  both before and after the cycle (rapid charging is a different setting on a
  different IOCTL, `0x83102148`), so the `0x83102150` write did not leak into the
  charge-mode word.
- **Independent handles**: observer A opens and closes a handle per query, and B
  and C ran in separate processes minutes later, so the readback is not a cached
  in-process value.
- **No hidden second state channel**: a bounded registry/file sweep of
  `HKLM|HKCU\SOFTWARE\Lenovo` found no night-charge state mirror — only
  `HKCU\SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin\NightChargeToast\NeverPopNightChargeToast = 1`
  (a UI-toast preference) and `BatteryChargeMode = Quick` (the charge-mode
  setting, unchanged). Independence therefore rests on re-open + separate
  implementations, which is what §3 uses.
- **Driver provenance**: `CreateFileW` on `\\.\EnergyDrv` succeeded, which is the
  authoritative existence proof for the interface; a service-key sweep of
  `HKLM\SYSTEM\CurrentControlSet\Services` found **no** service name containing
  `Energy`, and no `EnergyDrv.sys` under `System32\drivers`, so a service-list or
  driver-file check would have been a false negative here and was not used as one.

Not verified in this cycle (each needs its own bounded action):

- **Persistence across reboot / resume**: the setting is an EC/firmware setting,
  so it is expected to survive a power cycle, but no reboot was performed and no
  persistence claim is made.
- **Idempotent write**: writing the state the device is already in (`set on` while
  on) was not exercised; the CLI reports `previous == requested == actual` in that
  case only by construction of `p0.rs`, not by observation.
- **Mismatch/rollback branch of `p0.rs:179-205`**: never entered, because the
  firmware accepted both writes and the readback poll matched on the first sample.
  Forcing it would need a concurrent vendor write, which is not authorized.
- **Source ↔ binary correspondence**: the tested binary predates this session
  (`2026-11-05T14:23`); its behaviour matches the worktree source in §2, but it
  was not recompiled from the current tree during this item, and no build was run.
- **Rejection of non-contract selectors** (`native_ioctl.rs:73-79`) is untested on
  hardware; it is a validation branch, not a device behaviour.

## 7. Verdict

The night-charge channel is a **real, verified write channel** on this SKU:

- support probe `0x13` returns `0x00000011` (supported), matching the vendor
  `DoesSupportNightChargeMode = 1`;
- `SetNightChargeMode` reaches the EC and changes device state: selector
  `0x12` produced `Off`, selector `0x80000012` produced `On`, each confirmed by
  three independent readers reading a fresh handle;
- the write is reversible and was reversed: initial `On` → `Off` → `On`, with the
  final state word bit-identical to the baseline;
- the CLI's availability gate reports the feature `available` on this machine and
  its own readback poll reported `actual` equal to `requested` for both writes.

Remaining judgement calls for the parent (not decidable from this evidence):

1. Whether `battery status --json` should expose night-charge state; today the
   only CLI read path is `battery night-charge <toggle> --dry-run`.
2. Whether `"hardware":"unknown"` is intended at all: all 134 capability rows
   report `hardware = unknown` on this host (89 `available`, 33 `unsupported`,
   7 `limited`, 4 `external`, 1 `unavailable`), so the field carries no
   information here and is a general presentation gap, not a night-charge
   anomaly. It does not affect the `status`/`detail` pair this evidence relies on.
3. Suggested negative controls, in order of value: run the cycle with the Vantage
   add-ins stopped so the commit passes the gate without `--yes`; verify
   persistence across one reboot while the setting is `Off`; exercise the
   `p0.rs` rollback branch deliberately.

# Internal-panel refresh rate (`panel rate`) — independent target re-verification — 2026-11-05

Target: LENOVO 21VG (ThinkBook 14 G8+ IPH), `tb14p-ezra`, Windows `10.0.26300.9711`,
console session 1 (`ezra`, `dwm` + `explorer` running). Firmware block
`LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` on `ACPI\PNP0C14\GMZN_*`.

This document re-verifies `panel.refresh` readback, refusal and restore against a
**real program on the real panel**, recovers the mode list from the display
(`EnumDisplaySettingsExW` + `QueryDisplayConfig`) instead of a table, and records
one defect found live and fixed in source.

## 1. What was verified, and with what

| layer | artifact | identity |
| --- | --- | --- |
| program under test | `C:\Users\ezra\vantage-target\debug\vantage.exe` | `645b8b7be73672290f9fe2d0fbc702db39566e3a6d1666b5159f09270da3e2da`, 10413056 B, mtime `2026-11-05T14:23:35+08:00` (unchanged before/after the run) |
| independent observer | `VantagePanelObserve.cs` (this work) | `e3d541cd962b292841849b79176c36e9a9b48ecb6224dedf156ac4fb59ef32c8` |
| verification harness | `console-run.ps1` | `1acf88081222daf4f09d39182a774d58fa396474de988f62e77573de195871ae` |
| console launcher | `drive.ps1` | `8f5b7d5e913e719c06d72619a200f0cf9dc24d4e63ab426f896097aa814bd184` |

The observer is a separate process from the CLI, uses only
`GetDisplayConfigBufferSizes`/`QueryDisplayConfig`/
`DisplayConfigGetDeviceInfo`/`EnumDisplaySettingsExW`/`WTSGetActiveConsoleSessionId`,
and asserts that it runs inside the active console session (§5).

Source binding of the program: the target build tree
`C:\Users\ezra\vantage-machine-fix-source` holds
`crates/lctrl-hal-win/src/display.rs` = `a9d04d5d…` (byte-identical to this
worktree) and a `vantage-cli/src/lib.rs` whose `PanelRate` enum and
`PanelCommand::Rate` arm are byte-identical to this worktree's; the target copy
differs elsewhere only by sibling feature work (extra commands), and
`lctrl-core/src/panel.rs` differs only in an `EyeCareLevel` doc comment. The
display path exercised here is therefore the current source's display path.

## 2. The mode list comes from the display, not from a table

`EnumDisplaySettingsExW(device, index, 0)` over `\\.\DISPLAY1` returned
28 geometries at 32 bpp; **every** geometry advertises exactly the same rate
set. At the active geometry:

```text
3072x1920x32  rates = 30, 48, 60, 75, 100, 120
e.g. 1920x1080x32 / 2560x1600x32 / 1024x768x32  rates = 30, 48, 60, 75, 100, 120
```

Firmware (`LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA`, 22 `Active=TRUE` records, all
identical — the controller's documented de-duplication):

```text
InternalPanelHwId = 0x8ad7ae30
MinimumRefreshRate = 60   MaximumRefreshRate = 120   DefaultRefreshRate = 60   Mode = 1
```

### 2.1 Offered list vs. accepted list — the real mismatch

| list | members | source |
| --- | --- | --- |
| CLI accepts | `60`, `120`, `auto` | `PanelRate` `ValueEnum` (`vantage-cli/src/lib.rs`) |
| HAL admits for the current layout | `60`, `75`, `100`, `120` | firmware range `60..=120` ∩ enumerated set |
| driver advertises | `30`, `48`, `60`, `75`, `100`, `120` | `EnumDisplaySettingsExW` |

Consequences, split by what was measured and what is derived:

- **Derived, not exercised**: `75` and `100` Hz pass both admission gates
  (`requested_mode` checks firmware range, then requires the exact mode in
  `compatible_modes`, which is built from this very enumeration), so the HAL would
  commit them — yet the CLI's `ValueEnum` makes them unreachable. That is the
  offered-vs-accepted mismatch. It was not turned into a live write because no
  supported CLI shape can ask for those rates.
- **Derived from the same two sources**: `30` and `48` Hz are advertised by the
  driver but sit below the firmware minimum (`60`), so `requested_mode` rejects
  them with `InvalidArgument` before any display call. This run's refusal probes
  used values the CLI rejects at parse time, so none of them reached that check.

`auto` is not a rate: it resolves to `DefaultRefreshRate` (60 Hz) as a *fixed*
mode (`display.rs::requested_mode`), which is what the CLI prints.

The rate set is uniform across geometries, i.e. it is the display driver's
per-output list, not a panel-EDID timing table; the firmware block is the
narrower authority and the HAL correctly applies firmware-first, then
exact-enumeration.

## 3. Check results (one run, 36 checks)

Run: `2026-11-05T20:01:57Z` (local `2026-11-06 04:01`), console session 1,
observer PID 75568; transcript + `result.json` in the receipt table (§8).

| # | check | result |
| --- | --- | --- |
| 1 | binary digest pinned before/after | PASS |
| 2 | observer digest pinned | PASS |
| 3 | observer inside the active console session | PASS (`console=1 observer=1`) |
| 4 | exactly one active internal path | PASS (`paths=1 internal=1`) |
| 5 | enumeration contains the active mode | PASS (`120 ∈` set) |
| 6 | `Win32_VideoController` cross-check of the baseline rate | PASS (`Intel Arc B390 = 120`) |
| 7 | `--dry-run panel rate 60` reports `dry_run`, no `actual` | PASS (exit 0) |
| 8 | dry run leaves active **and** registry mode unchanged | PASS (`120`/`60`) |
| 9 | `panel rate 75 / 90 / 30 / 240` refused | PASS (exit 2, clap `invalid value`, no display call, mode unchanged) |
| 10 | unchanged request (`120` while at `120`) | PASS (exit 0, `previous=requested=actual=120`, no display change) |
| 11 | `--yes panel rate 60`: report `actual=60` | PASS (exit 0) |
| 12 | independent readback after commit | PASS (`EnumDisplaySettings` = 60) |
| 13 | only the refresh field changed (layout, position, rotation, flags, registry, path identity) | PASS |
| 14 | rate list unchanged by the transition | PASS (`30,48,60,75,100,120` before and after) |
| 15 | `--yes panel rate 120` (explicit restore): report `actual=120` | PASS (exit 0) |
| 16 | independent readback after restore | PASS (`120`) |
| 17 | restore changed only the refresh field | PASS |
| 18 | `DISPLAYCONFIG` target refresh equals the integer rate | **FAIL — harness strictness, see §3.1** |
| 19 | `--auto-restore` returns to the previous rate | **FAIL — real defect, §4** |
| 20 | final state equals the baseline | **FAIL — consequence of §4** |

`PASS` counts: 30/36. Six failures = two in #18 (60.001 / 120.001) and four from
the `--auto-restore` defect.

### 3.1 The two `DISPLAYCONFIG` failures are the observer's, not the product's

`QueryDisplayConfig` reports the target mode as a rational: `numerator/denominator`
= `60.001` and `120.001` when rounded to three decimals, while
`EnumDisplaySettingsExW` (the API the HAL reads) and the CLI report exactly
`60` and `120`. The harness compared the rational with exact integer equality;
that assertion is wrong for this output. The raw rational was not captured in
this run, so the exact pair is not claimed here. Reads and writes are unaffected:
the HAL never consults `DISPLAYCONFIG` refresh for admission.

## 4. Defect found live: `--auto-restore` cannot restore and leaves the new rate

Reproduction on the target (console session 1; baseline `120 Hz`, firmware
`Mode=1`, `DefaultRefreshRate=60`):

```text
vantage --json --yes --auto-restore panel rate 60
  exit 6
  {"error":{"kind":"firmware_rejected",
            "message":"firmware rejected request: panel refresh restore mismatch:
                       expected 120 Hz, actual Some(60) Hz", ...}}
  independent readback afterwards: 3072x1920x32 @ 60 Hz   (restore did not happen)
```

Root cause (`vantage-cli/src/lib.rs`, `execute_panel`, `PanelCommand::Rate`): the
arm captured `panel.refresh_mode()` and, when the policy was
`RefreshMode::Adaptive` (this machine: firmware `Mode=1`), built the restore
request as `PanelRefreshRequest::Auto`. `Auto` does **not** mean "the previous
rate": `display.rs::requested_mode` resolves `Auto` to
`capability.default_hz` = `DefaultRefreshRate` = **60 Hz**. With the previous rate
at 120 Hz the restore therefore asked for 60 Hz while the panel was already at
60 Hz: no transition, `actual` = 60, the mismatch guard fired — *after* the panel
had already been moved to the new rate. The adaptive special case is fiction:
the firmware refresh `Mode` has no write channel anywhere in this HAL, so no
request shape can restore adaptivity.

Fixed in source (this work, `vantage-cli/src/lib.rs` `PanelCommand::Rate`): the
policy capture and the `Adaptive → Auto` branch are gone; the restore always
replays the exact observed previous mode as
`PanelRefreshRequest::Fixed(previous.refresh_hz)`, keeping the `u16` conversion
and the mismatch guard (they are what caught the defect). In-code comment records
why `Auto` must never be a restore target. `PanelControl::refresh_mode()` stays in
the HAL — it is a legitimate read (and the only readable VRR signal, §6); only its
use in this arm is removed. No test pinned the old behaviour (there is no
panel-level `auto_restore` test in `vantage-cli/tests/*`), so nothing was weakened.

**This defect exists in the shipped binary behaviour, not only in the tree**: the
binary above was built `2026-11-05T14:23` from a tree whose panel arm is
byte-identical to ours, and it reproduced live.

## 5. Write path and admission (what the HAL actually does)

`crates/lctrl-hal-win/src/display.rs` (`WindowsPanelController`):

1. `refresh_capability()` reads `Minimum/Maximum/DefaultRefreshRate` + `Mode` from
   `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA WHERE Active = TRUE`, collapsing
   field-identical duplicate records and failing closed on disagreement.
2. `requested_mode()` rejects a rate outside the firmware range
   (`InvalidArgument`) and then requires an **exact enumerated native mode** for
   the current layout (`Unsupported … .exact_native_mode`). Only the four
   guaranteed per-entry fields (w/h/bpp/hz) are taken from the entry; position,
   orientation and target identity are inherited from the live mode.
3. `test_mode()` = `ChangeDisplaySettingsExW(CDS_TEST)`; a dry run stops here and
   reports `previous`/`requested` only.
4. Before a commit the path and current mode are re-read
   (`panel refresh state changed before commit; no write attempted`), then
   `apply_mode()` = `ChangeDisplaySettingsExW(flags = 0)` — dynamic mode only, no
   `CDS_UPDATEREGISTRY`; the persisted/registry mode is never written.
5. One immediate readback (`poll_readback(…, 1, Duration::ZERO, …)`), then
   `recover()`: rewrite the previous mode only if the panel is still observed at
   *this* request's rate; a third value or a changed path is reported as
   unverified instead of being overwritten.

Live fail-closed observations around that path:

- Non-console context (SSH network logon): `vantage --json --dry-run panel rate 60`
  → exit 5, `permission_denied`, "requires access to the active Windows desktop
  session". No silent success.
- After the topology changed under us (§7): `panel rate 120` →
  `unsupported: panel.internal.connected`. The tool refuses instead of writing to
  the external output.

Negative control for the admission rule that this run could **not** reach from the
CLI (the `ValueEnum` blocks every rate except 60/120/auto): the library test
`crates/lctrl-hal-win/tests/display_controller.rs::firmware_range_does_not_admit_a_mode_missing_from_native_enumeration`
— in-range rate absent from the enumerated set → `Unsupported
panel.refresh.120Hz.exact_native_mode`, **zero** apply calls and **zero**
`CDS_TEST` calls. Sibling guards:
`dry_run_tests_the_exact_mode_but_never_applies_it`,
`external_and_ambiguous_targets_are_rejected_before_any_write`,
`unchanged_request_still_checks_for_concurrent_mode_change`.

## 6. VRR (firmware `Mode`) — read yes, write no

- **Read**: `PanelControl::refresh_mode()` returns `RefreshMode::from_raw(Mode)`;
  live value `Mode = 1` (`Adaptive`), identical across all 22 active records.
  There is **no CLI surface** for it: no `panel refresh-mode` command, and
  `doctor` reports only `panel.refresh` "read succeeded" without the values. The
  value is used internally (until this work, only to pick the restore request).
- **Write**: none. `Mode` is a firmware WMI data block; no documented or observed
  method writes it, and the CLI writes only the desktop mode through
  `ChangeDisplaySettingsExW`. `panel rate auto` is a fixed `DefaultRefreshRate`
  request, not an adaptive/VRR request, and must not be documented as one.
- **Unverified**: whether the panel/driver keeps Adaptive-Sync active while the
  desktop is pinned at a fixed rate, and the physical refresh actually scanned
  out. `QueryDisplayConfig` exposes only the target rational (`120.001`), which is
  not VRR telemetry.

## 7. State left on the target and the topology change that intervened

1. The verification run moved the internal panel `120 → 60 → 120` (verified) and
   then the defective `--auto-restore` run left it at **60 Hz**. The persisted
   (registry) mode was **60 Hz at baseline and stayed 60 Hz throughout** — the
   writes are dynamic-only, so nothing persisted changed.
2. A repair attempt at `05:27` local (same command that had worked at `04:02`)
   failed with exit 5 `permission_denied`, `need: administrator`, before the
   restore could run. This is **unexplained** and is *not* attributed to any
   product behaviour; it coincides with the topology change below and did not
   recur (later failures use the honest `panel.internal.connected`).
3. From `05:32` local the internal panel is **not in the active topology**: the
   only active path is `\\.\DISPLAY5` = `S2719DGF`, technology `5` (HDMI),
   `3072x1920@144`, `internal=false` (the `GameViewer` virtual adapter). Only
   `DISPLAY\DELD0E6…` is present in `WmiMonitorID`; no internal monitor instance.
   `Win32_VideoController` still reports the Intel adapter at `60` — the internal
   panel keeps the rate left by §4 but cannot be addressed while it is inactive.
   **This topology change is external** (the panel was still active at `05:27`
   with a successful readback; no display-topology API is called anywhere in the
   panel write path).

To finish the rollback once the internal panel is active again:

```text
vantage --json --yes panel rate 120      # must run in the interactive console session
```

Then confirm with an independent read (expected `3072x1920x32 @ 120 Hz`).

## 8. Receipts (private originals stay in `C:\Users\ezra\vantage-panel-refresh-verify-20261106\`)

| file | length | SHA-256 |
| --- | --- | --- |
| `transcript.txt` | 9271 | `566fa38f2937032e9d3033a762b3ce94fdba161d077c33819ecabb25ebedc91f` |
| `result.json` | 55255 | `a81afecf26666f8c998c6224728ef346857b111d76eec89733df59b5ef0c2a05` |
| `dryrun.out` | 529 | `eb38c461533ebe225bae14a8874e77e9fd3221f8924d71263f6526de84e7ae5f` |
| `noop.out` | 782 | `cca9cd0a7bae6b1ecc0907dfcdf80ff28c47391ba32adcbaf3b7eab76724e913` |
| `commit.out` | 780 | `34f526072a9cbf0abf9ba0f9352a9d5da4c7e9cc6a68e9475b3dca2dd87a2d51` |
| `restore.out` | 781 | `b761bcac70fa16a3b6c802cd271c5b16a15674e1bbbed96b1c493e75a475edcd` |
| `auto-restore.out` | 0 | `e3b0c442…` (empty) |
| `auto-restore.err` | 225 | `928bed2eae6bddf9b02077a0f59934ac37e10ded04f7ddbc3c0e6d0395a57a67` |
| `dryrun.err`/`commit.err`/`restore.err` | 0 | `e3b0c442…` |
| `refuse-75.err` | 111 | `c9b17cac91be5a7f533210cc3d643823ea25d47ab8f21f8dd9300f7b42fdec90` |
| `refuse-90.err` | 111 | `c88df5dcb316e10aed464bba6fd34464721a1bd82577f4dc4b9a4b699bb28e4c` |
| `refuse-30.err` | 111 | `3cff8885515588af1c04763dcc414d39697b66e6d7e53351b8b71384ea737aae` |
| `refuse-240.err` | 112 | `3ff063e7efac8e7e32a9dc6873a736b46dbff8a22e48d4b61583c1f72fbd8543` |

Also on the target: `diag-transcript.txt`, `topo-transcript.txt`,
`restore-rescue-transcript.txt`, `RESTORE_DONE` (= `wrong_state_60`), and the
observer/harness sources. Private scratch scripts live in `C:\Users\ezra\probe*.ps1`,
`restore2.ps1`, `diag.ps1`, `topo.ps1`.

## 9. Not covered / next actions

1. **Re-verify the fix live** — needs a rebuild of the binary (this work does not
   build): `vantage --json --yes --auto-restore panel rate 60` must exit 0,
   print `restored panel refresh: 120 Hz (verified)`, and leave the panel at 120.
2. **Add the missing regression bound.** No CLI test covers panel
   `--auto-restore`, which is why this shipped. Cheapest shape: a
   `PanelControl` fake in `vantage-cli/tests/cli_contract.rs` whose
   `refresh_capability` reports `default_hz = 60` while the current mode is 120
   and whose `refresh_mode` is `Adaptive`; assert that the restore request seen by
   the fake is `Fixed(120)` and that the JSON `auto_restore` string reports 120 Hz.
   Not added here because this seat does not build, and an uncompiled test in a
   shared file would block sibling work.
3. `panel rate auto` was **not** re-exercised in this run (the panel went inactive
   first); the 2026-10-27 acceptance remains its only live evidence
   (`actual = 60`, firmware `DefaultRefreshRate = 60`).
4. `InternalPanelHwId` is `0x8ad7ae30` today; `docs/04-peripherals.md` §3.1 states
   `0x8A000A00`. Either the doc value is stale or it was read through a different
   property. Not changed here — the owner of that table should re-read it.
5. Still unexercised: multi-monitor/dock topologies (only a single active internal
   path is admitted), external-controller races, process crash mid-transition, and
   persistence of a fixed rate across a topology change or a firmware adaptive
   re-assertion.
6. Physical panel timing was not instrumented; a successful `ChangeDisplaySettingsExW`
   plus a matching readback is the strongest claim made here.

## 10. CLI reports as observed (verbatim fields, receipt files in §8)

```text
dry-run     mode=dry_run  previous=120  requested=60   actual=(absent)
no-op       mode=commit   previous=120  requested=120  actual=120
commit      mode=commit   previous=120  requested=60   actual=60
restore     mode=commit   previous=60   requested=120  actual=120
```

All four reports name the same path identity every time — device `\\.\DISPLAY1`,
adapter low `108074`, source `0`, target `8388688`, geometry `3072x1920x32`,
position `(0,0)`, rotation `default` — so the only field that moved across the
commit and the restore is `refresh_hz`.

## 11. Changes made by this work

| path | change |
| --- | --- |
| `crates/vantage-cli/src/lib.rs` (`execute_panel`, `PanelCommand::Rate`) | restore always replays `Fixed(previous.refresh_hz)`; the `RefreshMode::Adaptive → Auto` branch and the policy capture are removed; the reason `Auto` is not a restore target is recorded in-code |
| `docs/04-peripherals.md` §3.1 | one paragraph: `--auto-restore` replays the observed fixed rate; the firmware `Mode` has no write channel, so adaptivity cannot be restored, and `auto` (firmware default) is not a restore target |
| `docs/C-acceptance.md` #22 | records the live defect, the correction, that post-fix live acceptance is still outstanding, and the `EnumDisplaySettings`-based mode-list check |
| `docs/evidence/re-panel-refresh-20261105.md` | this document |

No HAL trait, public type, generated artifact or display-write code was changed;
`display.rs` is untouched. `PanelControl::refresh_mode()` remains in the HAL.
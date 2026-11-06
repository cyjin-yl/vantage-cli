# `--dry-run` commit-path audit: every mutating command

Scope: `crates/vantage-cli/src/lib.rs` (CLI dispatch and every handler that can reach a hardware
write), the Windows HAL writers those handlers call, `crates/lctrl-hal/{src,win,linux}/src/radio.rs`,
and the `daemon` verbs in `vantage/src/main.rs` (the platform binary that intercepts them).

**No binary, test, or target probe was run by this item** (execution is out of scope for it).
Every verdict below is source-bound: read the deciding branch, read the backend that branch
guards. Anything needing a live observation is marked as such and handed to the parent (§6).

## 0. Snapshot, and one correction to the dossier

The audit started from `docs/evidence/re-cli-surface-20261105.md` §7, which reported three
subcommands whose own `--dry-run` field shadows the global flag and concluded that
`vantage --dry-run update local-driver apply …` "can COMMIT a hardware mutation".

**That conclusion is wrong, and this audit's independent reading of the pinned clap source
refutes it** (§1). The shadow was real (a duplicate arg id) but it did not lose the value: clap
copies matched global values between levels by id, and the local field had the same id
(`dry_run`). The old code was therefore *correct by accident*. The independent confirmation of
this came from `vantage-re-wave6-1`, who de-shadowed the five duplicate ids while this audit was
running; the two readings agree.

What survives as genuinely unsafe is a different set: commands with **no `ApplyMode` in the path
at all** (§3 F5/F6/F7), plus a gating gap (§3 F8).

The companion document is `docs/evidence/re-dryrun-shadow-20261106.md` (`vantage-re-wave6-1`): its
§2 is the clap value-propagation chain (same conclusion, reached independently), its §4 is the
arg-id collision survey (the five shadowed fields plus a clean `json`/`yes`/`auto_restore`), and its
§4 scope paragraph defers the no-mode class to this document. Read them as a pair: *a subcommand
that does not collide can still fail to honour `--dry-run`* — `radio` did exactly that (§3 F5).

| item | value |
| --- | --- |
| worktree | `/tank/shared/projects/vantage/replacement/target/bios-docs-20261030` (shared, dirty, ~25 workers) |
| `git rev-parse --short HEAD` | `c07c775` (`docs: recon privacy-camera + OSD channels`) |
| `pinned clap` | `clap_builder 4.6.2` (`Cargo.lock:924-927`), `clap_derive 4.6.1` |

`crates/vantage-cli/src/lib.rs` changed continuously during the audit (9 distinct hashes in ~1 h,
8.1 k → 9.0 k lines). Snapshot at hand-off, on which this item's one hunk was re-read and confirmed
present: `lib.rs` sha256
`d6602fec4a4a3941aab339cee3c9ca77f61ca774b965fd3b22058c9019cad63c`. Line numbers here are
indicative; **anchor text is authoritative.**

## 1. How `--dry-run` actually resolves (and why the shadow did not lose it)

### 1.1 The single CLI decision site

`execute_with_services` resolves the mode once:

```rust
let apply = if cli.dry_run {
    ApplyMode::DryRun
} else {
    ApplyMode::Commit
};
```

(Snapshot `9014349…`: `lib.rs:1853-1858`.) Before `wave6-1`'s change the same site was
`if cli.dry_run || local_dry_run` with `local_dry_run` matching only
`ProfileCommand::Apply { dry_run: true }`; that special case is gone now that
`ProfileCommand::Apply` no longer declares its own flag.

Everything downstream must read this value. Three subtrees (`FixCommand::Run`,
`SceneCommand::Test`, `LocalDriverCommand::{Apply,Rollback}`) plus `ProfileCommand::Apply` used to
declare their own `#[arg(long)] dry_run` and read only that field.

### 1.2 Why the shadowed field still received the global value

Two distinct clap mechanisms — conflating them is what produced the dossier's error:

1. **Declaration propagation** (`clap_builder-4.6.2/src/builder/command.rs:4751-4762`,
   `_propagate_global_args`) pushes a *copy of the `Arg` definition* into each subcommand and
   **skips** any subcommand that already has an arg with that id:
   ```rust
   for a in self.args.args().filter(|a| a.is_global_set()) {
       if sc.find(&a.id).is_some() {
           debug!("Command::_propagate skipping {:?} to {}, already exists", a.id, sc.get_name());
           continue;
       }
       sc.args.push(a.clone());
   }
   ```
   This is the *only* thing the shadow broke: the subtrees did not get their own copy of the
   global `Arg` definition.

2. **Value propagation** (`command.rs:4349-4378` → `ArgMatcher::propagate_globals`,
   `src/parser/arg_matcher.rs:47-91`). `_do_parse` collects the ids of every global arg declared
   at any level (`get_used_global_args`, `command.rs:4335-4347`) and then
   `fill_in_global_values` walks the matcher tree, accumulating one `MatchedArg` per **id** into a
   shared `vals_map`, choosing by `ValueSource` precedence
   (`src/parser/matches/value_source.rs:1-17`: `DefaultValue < EnvVariable < CommandLine`), and
   finally inserting the winning entry into *every* level (`arg_matcher.rs:88-90`). Because the
   recursion descends before that insert, the **leaf's explicit value also ends up in the root
   matcher** — the documented behaviour of `Arg::global`
   (`src/builder/arg.rs:876-889`): *"Global arguments only propagate down, not up (to parent
   commands), however their values once a user uses them will be propagated back up to parents …
   it doesn't matter where the user uses the global argument."*

Consequence: the leaf field was keyed `dry_run`, the root global was keyed `dry_run`, so the
matcher handed the root's explicit `--dry-run` to the leaf field, and the leaf's own `--dry-run`
back to the root. All three argument positions therefore resolved to `DryRun` *before* the fix:

| invocation (pre-fix code) | root `Cli.dry_run` | leaf field | resulting mode |
| --- | --- | --- | --- |
| `vantage --dry-run fix run <issue>` | `true` | `true` (propagated by id) | DryRun |
| `vantage fix --dry-run run <issue>` | `true` (propagated up) | `true` | DryRun |
| `vantage fix run <issue> --dry-run` | `true` (propagated up) | `true` (own occurrence) | DryRun |

### 1.3 Why it was still worth fixing (what the shadow really cost)

- Correctness rested on an **undocumented coincidence**: the local field's clap id had to stay
  exactly `dry_run`. Renaming the field, giving it `id = "no_write"`, or making the local flag
  `global = false` would have silently converted the middle and trailing positions into commits.
- `--help` for those subtrees showed a second, independent `--dry-run`, and arg introspection
  (completions, conflict detection) saw two different args.
- `wave6-1`'s fix (delete all five duplicate fields; one global id) removes the coincidence, so the
  behaviour no longer depends on id equality. **That fix is now the tree's design**, so the
  regression risk moved: the trailing form now depends on clap's up-propagation instead of a local
  field, which is why §6 item 1 (both argument positions) is a mandatory live check.

## 2. Verdict table

Legend: **SAFE** = a dry-run request cannot reach a write (branch read); **UNSAFE** = a dry-run
request can write; **HARDENED** = was fragile, de-shadowed and now single-sourced.

### 2.1 Families that thread the resolved `apply` into the backend

Traced twice per row: the CLI passes the resolved `apply` (never a constant), and the backend
returns before its write on `DryRun` (§4).

| # | command | write path given the resolved mode | verdict |
| --- | --- | --- | --- |
| 1 | `battery charge-mode <mode>` | `battery.set_charge_mode(mode, apply)` | SAFE |
| 2 | `battery night-charge <on/off>` | `battery.set_night_charge(enabled, apply)` | SAFE |
| 3 | `power scheme apply/set <scheme>` | `power.apply_power_mutation(mutation, apply)` | SAFE |
| 4 | `power saver-once` | `apply_power_mutation` / `set_epp_percent_ac_dc` / `set_brightness` with `apply`; restores behind `apply == Commit` | SAFE |
| 5 | `perf mode <mode>` | `performance.set_performance_mode(mode, apply)` | SAFE |
| 6 | `perf pl1/pl2 <watts>` | `tuning.set_power_limit(kind, µW, apply)` | SAFE |
| 7 | `perf fan auto/manual/fullspeed/smart/curve` | `fan.set_fan_mode(…, apply)` | SAFE |
| 8 | `perf boost/throttle <pids>` | `boost_process`/`throttle_process` → `transact(apply)` | SAFE |
| 9 | `tune profile apply <name>` | single resolution now; dry run returns the plan before `snapshot_tune_plan`/`write_tune_state`/`apply_tune_plan` | HARDENED |
| 10 | `tune restore` | `restore_tune_setting(…, apply)`; state file removed only under `Commit` | SAFE |
| 11 | `tune pl1 <mw>` | `apply_limit_sequence(tuning, &requests, apply)` | SAFE |
| 12 | `tune epp <value>` | `tuning.set_epp(value, apply)` | SAFE |
| 13 | `tune turbo <state>` | `power.set_turbo(enabled, apply)` | SAFE |
| 14 | `snapshot capture` | baseline file written only when `apply == Commit` | SAFE |
| 15 | `snapshot restore` | every `set_*` receives `apply`; rollback list only under `Commit` | SAFE |
| 16 | `panel brightness <pct>` | `panel.set_brightness(percent, expect_current, apply)` | SAFE |
| 17 | `panel rate <hz>` | `panel.set_refresh_rate(requested, apply)` | SAFE |
| 18 | `panel color …` | refuses: `"panel.color.write"` | SAFE (by refusal) |
| 19 | `panel super-resolution <state>` | `features.set_super_resolution(…, apply)` | SAFE |
| 20 | `panel overdrive …` | refuses: `"panel.overdrive.write: …"` | SAFE (by refusal) |
| 21 | `panel eye-care <level>` | `features.set_eye_care(target, apply)` | SAFE |
| 22 | `kbd backlight <level>` | `keyboard.set_backlight(level, effect, apply)` | SAFE |
| 23 | `kbd fnlock / fn-ctrl-swap <state>` | `execute_persistent_bios_toggle`: `DryRun` returns before `confirm_risky_change`/`stage_and_save` | SAFE |
| 24 | `kbd winlock <state>` | refuses: `"firmware rejects the Win-key methods …"` | SAFE (by refusal) |
| 25 | `touchpad on/off` | `touchpad.set_touchpad(requested, apply)` | SAFE |
| 26 | `touchpad recover` | `touchpad.recover_touchpad(apply)`; backend gate `runtime_devices.rs:823` | SAFE |
| 27 | `privacy cam on/off [--runtime / --persistent]` | `privacy.set_camera(requested, apply)` or BIOS toggle | SAFE |
| 28 | `privacy mic <state>` | `privacy.set_microphone(requested, apply)` or BIOS toggle | SAFE |
| 29 | `privacy fingerprint <state>` | persistent BIOS toggle (dry run returns first) | SAFE |
| 30 | `bios set <name> <value> --save` | `DryRun` returns a `ChangeReport::dry_run` before `confirm_risky_change`/`stage_and_save` | SAFE |
| 31 | `bios discard` | `DryRun` returns before `bios.discard()` | SAFE |
| 32 | `bios defaults` | `DryRun` returns before `bios.load_defaults()` | SAFE |
| 33 | `bios save` | refuses: `"bios.save.global-buffer"` | SAFE (by refusal) |
| 34 | `bios password set/clear` | refuses: `"bios.password.write"` | SAFE (by refusal) |
| 35 | `usb always-on <state>` (`--persistent` → BIOS `AlwaysOnUSB`) | `usb.set_usb_ac_charging(enabled, apply)`; persistent path calls `execute_bios(…, apply, …)` | SAFE |
| 36 | `usb charge-on-battery <state>` | `usb.set_usb_battery_charging(enabled, apply)` | SAFE |
| 37 | `sense lock-on-leave / wake-on-approach / pause-video / camera-assist / approach-auto-adjust / leave-auto-adjust / attention-tracking / kbd-light-auto / reset` | `apply_sense_features(…, apply)` → `sense.set_sense_feature(feature, apply)`; restore plan built only under `Commit` | SAFE |
| 38 | `audio dolby <profile>`, `audio noise-cancel <mode>`, `audio volume --percent`, `audio mute --state` | `audio.set_dolby_profile(target, apply)`, `noise_cancel.set_noise_cancel_mode(target, apply)`, endpoint volume/mute with `apply` | SAFE |
| 39 | `osd enable/disable/test/show` | `apply → dry_run` bool → `osd.set_fn_osd(enabled, dry_run)`, `osd.show(…, dry_run)`; backend gates `osd.rs:89`, `fn_osd.rs:138` | SAFE |
| 40 | `hotkey register/unregister <id>` | `apply → dry_run` → `hotkey.register(id, vk, dry_run)`; gate `hotkey.rs:160` | SAFE |
| 41 | `update local-install <pkg>` | `execute_local_package(…, mode)`; backend returns before elevation/`run_installer` (`local_package.rs:288`) | SAFE |
| 42 | `update local-driver apply <inf> <hwid>` | resolved `mode` threaded through `execute_update` → `execute_local_driver(local_update, mode, command)` → backend `DryRun` gate `local_updates.rs:79` | HARDENED |
| 43 | `update local-driver rollback <device…>` | same, gate `local_updates.rs:150` | HARDENED |
| 44 | `fix run <issue>` | resolved `mode` → `machine_fix.fix(issue, mode)`; gate `machine_fix.rs:124` | HARDENED |
| 45 | `scene test <event> [--rule <id>]` | resolved `mode` → `NativeScenePolicy::apply_*`; gates `scene_policy.rs:219/263/301/338` | HARDENED |
| 46 | `radio off <kind> [--force]`, `radio on <kind>` | radio owner added `apply: ApplyMode` to `RadioControl::radio_set_state` and a `DryRun` return before `SetStateAsync`; handler now `execute_radio(radio, command, apply)` and both arms pass it (`lib.rs:7355-7418`, dispatch `:2107`) | FIXED by owner (§3 F5) |
| 47 | `daemon start`, `daemon install` | `vantage/src/main.rs:109-110` passes `cli.dry_run`; branches at `:425`, `:492` before spawn/write | SAFE |
| 50 | read-only verbs: `info`, `doctor`, `capability`, `scan`, `snapshot diff`, `wifi security/history`, `radio status`, `fix scan/rules/track`, `scene list/track`, `update check/history`, `magicbay detect`, `magicbay lte status`, `perf temp/*`, `perf status`, `perf policy`, `perf dbdc get`, `perf gpu status`, `perf top`, `tune telemetry/watch`, `battery status/health/adapter/temporary-mode`, `audio status/endpoints`, `sense status`, `privacy status`, `bios list/get/password status|verify`, `hotkey list/status/trigger`, `completions` | no writer is reachable | SAFE |

### 2.2 Mutating verbs that a dry-run request can still commit

| # | command | verdict | class |
| --- | --- | --- | --- |
| 46 | `radio off` / `radio on` | **was UNSAFE**, FIXED by the radio owner during this audit (source-complete, unverified) | no `ApplyMode` in the path at all; could sever the live session |
| 47 | `wifi clear-history` | **UNSAFE (low)**, open (F6) | no `ApplyMode` in the path at all; clears the HAL's in-memory observation buffer |
| 48 | `hotkey unregister <id>` | **UNSAFE (low)**, open (F6b) | `HotkeyControl::unregister(id)` has no dry-run parameter, so the release happens on a dry-run request |
| 49 | `daemon stop` | **UNSAFE (low/med)**, open (F7) | `dry_run` is accepted and never read; the IPC `Stop` request is sent |

## 3. Findings

### F1-F4 — the four shadowed subtrees: no live bypass; structural hardening (CONFIRMED SAFE)

Sites (pre-fix snapshot): dispatch `execute_fix_run(…, dry_run)`,
`execute_scene_test(…, dry_run)`, `execute_local_driver(local_update, command)` and the two local
`let mode = if dry_run { … }` reconstructions inside `execute_local_driver`.

Settlement per §1.2: the value was not lost, because the local field and the global flag shared the
id `dry_run` and clap's value propagation is id-keyed in both directions. **No commit under a
dry-run request could be shown or derived for these four commands.** The dossier's "can COMMIT a
hardware mutation" was a misreading of mechanism 1 as if it were mechanism 2.

Residual defect that was real, and is now fixed (`vantage-re-wave6-1`, source-complete,
unverified): correctness depended on that id coincidence, and the duplicate declaration made the
subtree's `--dry-run` a second, unrelated arg. Fix applied there: the five duplicate fields
(`FixCommand::Run`, `SceneCommand::Test`, `LocalDriverCommand::{Apply,Rollback}`,
`ProfileCommand::Apply`) are deleted, `local_dry_run` is gone, and the handlers take the resolved
`mode: ApplyMode` directly. I did not re-patch these sites.

### F5 — `radio off|on` had no dry-run path at all (was the highest severity; now fixed by its owner)

Pre-fix: dispatch `execute_radio(services.radio, command)` took no mode and
`RadioControl::radio_set_state(kind, state, allow_session_cut)` had no mode parameter, so
`vantage --dry-run radio off wifi --force` switched the radio off — on a remote session, the
operator's own transport, while they believed they had asked for a plan. `RadioTransition` also had
no way to express "nothing was written" (`observed` was always a real observation).

The radio owner (`vantage-re-wave2-2`) took the interface route rather than a CLI-side early
return, so the HAL cannot write on a `DryRun` call at all: `apply: ApplyMode` on the trait,
`RadioTransition::mode` + `observed: Option<RadioState>` (`None` on a dry run),
`verified() == mode == Commit && observed == Some(requested)`, WinRT `SetStateAsync` unreachable on
`DryRun`, and the session-cut guard still evaluated during a dry run so the refusal the commit would
hit is reported instead of a plan. **Source-complete, unverified — treat as "cannot be trusted to
compile" until the parent builds.**

### F6 — `wifi clear-history` ignores `--dry-run` (open, low)

`execute_wifi` calls `wifi.clear_wifi_security_history()?` with no mode. State is memory-only
(nothing persisted), so severity is low, but the flag is silently ignored and the JSON output does
not distinguish plan from action. Not patched: "clear a local cache" is a semantic choice (refuse
under dry run vs. perform it as non-hardware housekeeping) that belongs to the wifi channel's owner.

### F6b — `hotkey unregister` ignores `--dry-run` (open, low)

`HotkeyControl::register` takes `dry_run: bool` (`crates/lctrl-hal/src/hotkey.rs:36`) and its
Windows implementation returns early (`crates/lctrl-hal-win/src/hotkey.rs:160`), but the sibling
operation does not: `fn unregister(&self, id: u32) -> Result<bool>`
(`crates/lctrl-hal/src/hotkey.rs:39`), called from `execute_hotkey` as `hotkey.unregister(id)?`. So
`vantage --dry-run hotkey unregister <id>` releases the registration. Severity is low because a
registration is process-scoped (it dies with the CLI process that created it), but the asymmetry is
exactly the class this audit was asked to find, and the fix (a `dry_run` parameter on the trait
method, or a CLI-level plan report) changes a shared interface, so it is held for the hotkey owner.

### F7 — `daemon stop` ignores `--dry-run` (open, low/med)

`vantage/src/main.rs:106-108` receives `dry_run` in `execute_daemon_command` and does not read it
for `Stop`, so `vantage --dry-run daemon stop` really stops the resident daemon. Not patched: the
daemon owner is editing that file. `daemon start`/`install` are correct (`:425`, `:492`).

### F8 — `command_is_mutating` did not cover the mutating surface (PATCHED here)

`command_is_mutating` gates `preflight_control_conflicts` — the vendor-controller refusal that
demands `--yes` (gate `lib.rs:1860`, predicate `lib.rs:2155`, preflight `lib.rs:2291`). It was
written before `update`, `fix`, `scene`,
`radio`, `usb`, `sense`, `audio`, `osd`, `hotkey` grew write verbs, so those paths committed with no
confirmation prompt and no concurrent-vendor warning.

Patched (this item, `crates/vantage-cli/src/lib.rs`, function `command_is_mutating`, the final arm
chain): added `Command::Usb`, `Command::Sense`, `Command::Osd`, `Command::Hotkey`, `Command::Update`,
`Command::Fix`, `Command::Scene`; extended the existing `Command::Audio` arm with
`Dolby`/`NoiseCancel` (it listed only the new `Volume`/`Mute` forms). `Command::Radio` was already
added by the radio owner.

Per-command classification and why — **only forms that can write are included**; a read-only form
must not start demanding `--yes`:

| family | included (mutating) | excluded (read-only / never writes) | reason |
| --- | --- | --- | --- |
| `usb` | `AlwaysOn { .. }`, `ChargeOnBattery { .. }` | — (there is no `usb status`; `Command::Usb { command: None }` is an `InvalidArgument` error) | both call `set_usb_*_charging`; `--persistent` goes through the BIOS transaction |
| `sense` | `LockOnLeave`, `WakeOnApproach`, `PauseVideo`, `CameraAssist`, `ApproachAutoAdjust`, `LeaveAutoAdjust`, `AttentionTracking`, `KbdLightAuto`, `Reset` | `Status` | every included variant calls `set_sense_feature`; `Status` reads |
| `audio` | `Volume { percent: Some(_) }`, `Mute { state: Some(_) }`, `Dolby { .. }`, `NoiseCancel { .. }` | `Volume { percent: None }`, `Mute { state: None }` (read forms), `Endpoints` | the `Option` presence distinguishes write from read for volume/mute; `Dolby`/`NoiseCancel` always write |
| `osd` | `Enable`, `Disable` | `Test`, `Show { .. }` | `Enable`/`Disable` write `LenovoFnAndFunctionKeys` control codes; `Test`/`Show` only paint our own overlay window and cannot fight a vendor controller |
| `hotkey` | `Register { .. }`, `Unregister { .. }` | `List`, `Status`, `Trigger { .. }` | register/unregister change the live hotkey table; `Trigger` is documented as a dry observation and only reads `registered()` |
| `radio` | `Off { .. }`, `On { .. }` (added by the radio owner) | `Status` | — |
| `update` | `LocalInstall { .. }`, `LocalDriver { command: Apply / Rollback }` | `Check`, `History` (read), `Download`/`Install`/`Ignore`/`Rollback`/`Schedule` (return `Unsupported` without writing) | — |
| `fix` | `Run { .. }` | `Scan`, `Rules` (command `None`), `Track` | `Run` starts/stops a Windows service |
| `scene` | `Test { .. }` | `List`, `Track` | `Test` applies the firing rule |

Deliberately **not** added:

- `Command::Snapshot { command: Some(SnapshotCommand::Capture) }` — writes a baseline file under
  the user's snapshot directory, not hardware; `Snapshot::Restore` was already listed.
- Anything already in the list stays, including `PanelCommand::Color`/`Overdrive` and
  `KbdCommand::Winlock`, which always refuse. That is pre-existing over-inclusion (a refusal never
  writes); trimming it is a separate contract change and would alter which commands demand `--yes`.
- `Command::Daemon` — the platform binary intercepts daemon verbs (`vantage/src/main.rs:618/649/845`
  call `handle_daemon_command` before `execute_with_services`), so `execute_with_services` never
  sees them and adding them here would be dead code.

**User-visible impact of F8 (the part to watch):** the gate is
`if apply == ApplyMode::Commit && command_is_mutating(&cli.command)` (`lib.rs:1742`), so nothing
changes for dry runs (they skip it), and nothing changes when no vendor controller is active
(`active_vendor_controllers()` empty). When a controller *is* active and `--yes` is absent, the
newly covered commands now fail with `LctrlError::InvalidArgument` →
**exit code 2** (`crates/lctrl-core/src/error.rs:25-35`), stderr
`stop <controllers> before writing, or re-run with --yes to accept concurrent-control risk`
(plus `conflicting vendor control application(s) detected: …` when interactive), where before they
ran and exited 0 on success (or with their own error: 3 unsupported, 4 channel unavailable,
5 permission denied, 6 firmware rejected, 7 verify mismatch). Adding `--yes` restores the old
behaviour. That is an intentional safety change, not a regression.

## 4. Backend evidence: each Windows writer returns before writing on `DryRun`

A CLI-level verdict is worth only what the backend gate is worth, so each distinct writer was read.
All of these return (or refuse) before their first mutating call:

| backend writer | gate |
| --- | --- |
| `local_updates.rs` `apply_local_driver_update` / `rollback_local_driver` | `:79` / `:150` — return with `actual: None` before `install_driver` / `rollback_driver` |
| `machine_fix.rs` `fix` | `:124` — returns before `apply_repair` and `persist_ledger_entry` |
| `scene_policy.rs` `apply_power` / `apply_panel` / `apply_fan` / `apply_performance` / `unavailable` | `:219` / `:263` / `:301` / `:338` / `:374` via `report_dry_run` |
| `local_package.rs` `apply_local_package` | `:288` — returns before the elevation check and `run_installer` |
| `p0.rs` `set_charge_mode` / `set_night_charge` | `:229` / `:183` |
| `battery_sources.rs` (`night_charge`, `charge_mode`) | pure delegation forwarding `apply` (`:104`, `:112`) |
| `performance_p0.rs` `set_performance_mode` | `:147` |
| `thermal.rs` `set_fan_mode` | `:272` |
| `power_tuning.rs` `set_index_ac_dc` / `apply_source_pair` / `set_epp` | `:150`, `:176`, `:279` (`set_power_limit` delegates to the same helper) |
| `power_p0.rs` `activate` / `set_value_for_scheme` | `:163` / `:244` |
| `process.rs` `transact` (boost/throttle/restore) | `:217` |
| `brightness.rs` `set_brightness` | `:344` |
| `display.rs` `set_refresh_rate` | `:206` |
| `panel_features.rs` `set_eye_care` / `set_super_resolution` | `:1271` / `:1319` |
| `kbd_backlight.rs` `set_backlight` | `:222`; `WindowsKeyboardSources::set_backlight` (`:304`) is a pure delegation |
| `peripherals.rs` `set_backlight` | `:121` |
| `runtime_devices.rs` `set_group` / `set_device` / `recover_touchpad` | `:515` / `:603` / `:823` |
| `sense.rs` `set_sense_feature` | `:112` |
| `usb.rs` `set_usb_ac_charging` / `set_usb_battery_charging` | `:56` / `:81` |
| `dolby.rs` `set_dolby_profile` | `:124` |
| `noise.rs` `set_noise_cancel_mode` | `:101` |
| `hotkey.rs` `register`, `osd.rs` `show`, `fn_osd.rs` `set` | `:160`, `:89`, `:138` (bool `dry_run`) |
| BIOS writers (`stage_and_save` / `discard` / `load_defaults`) | no mode of their own — the CLI is the only gate, and it returns before calling them (rows 23, 30-32, 35) |

## 5. Changes landed during this item

| change | owner | state |
| --- | --- | --- |
| F1-F4: five duplicate `dry_run` fields deleted, `Cli.dry_run` is the single resolution, handlers take `mode: ApplyMode` | `vantage-re-wave6-1` | source-complete, unverified |
| F5: `radio_set_state` gained `ApplyMode`, `RadioTransition` gained `mode` + `observed: Option<_>`, WinRT write unreachable on dry run, `Command::Radio` added to the mutating predicate | `vantage-re-wave2-2` | source-complete, unverified |
| F8: `command_is_mutating` coverage for `usb`, `sense`, `osd`, `hotkey`, `update`, `fix`, `scene` (+ `audio` `Dolby`/`NoiseCancel`) | this item | applied at the tail of `command_is_mutating`; source-complete, unverified |

No other file was touched by this item. The audit changed no declarations, no clap attributes, and
no backend.

## 6. Parent checks and negative controls

None of these was run here.

1. **Both argument positions, for every command that gained a mode or lost a local field.** This is
   now the load-bearing check for F1-F4/F5, because the tree relies on clap's value propagation
   rather than a local field:
   - `vantage --dry-run fix run <issue> --json` and `vantage fix run <issue> --dry-run --json` must
     both report `mode == "dry_run"`, `actual == null`, exit 0, and leave `vantage fix track`
     unchanged (ledger count before == after).
   - same pair for `scene test <event>`, `update local-driver apply <throwaway.inf> <hwid>`, and
     `radio off <kind>` (the last only against a kind that does not carry the session; it needs a
     `RadioControl` fake — `RadioCommand` never declared a shadowed local flag, so this one checks
     the owner's new mode wiring, not a de-shadowing).
   - the middle form `vantage fix --dry-run run <issue>` must also stay a dry run.
2. **Negative control that the local flag is really gone and the global still commits when asked to
   commit**: `vantage fix run <issue> --yes` (no `--dry-run`) must commit exactly once; and
   `vantage fix run <issue> --dry-run=false` must be a parse error (SetTrue takes no value), not a
   silent commit.
3. **F8 exit-code change, both directions**: with a vendor controller active (Vantage/PCManager
   running) and no `--yes`, `vantage usb always-on on`, `vantage sense lock-on-leave on`,
   `vantage fix run <issue>`, `vantage update local-install <pkg>`, `vantage audio dolby music`
   must exit **2** with the "re-run with --yes" message; the same commands with `--dry-run` must
   still exit 0 and be a dry run; with `--yes` they must behave as before the patch. Read-only
   neighbours must be unaffected: `vantage sense status`, `vantage audio status`,
   `vantage audio volume` (no `--percent`), `vantage hotkey list`, `vantage fix scan`,
   `vantage scene list`, `vantage update check` must **not** start demanding `--yes`.
4. **Device-state readback for the driver path** (the one hardware family here):
   `Get-PnpDeviceProperty -InstanceId <id> -KeyName DEVPKEY_Device_DriverVersion` (and
   `…DriverDate`) before and after the dry run must be identical, and identical after the paired
   commit control.
5. **Detector-stub control for the F8 arms** (unit-level, no hardware, no vendor controller
   needed). Not present in `wave6-1`'s `dry_run_resolution.rs` by their deliberate choice — their
   file is uncompiled/unverified and a trait-shape mismatch there would fail the whole test binary
   and take the eight mode-resolution guards with it — so this belongs to whoever runs the suite.
   Pinned shape: stub `lctrl_hal::ControlConflictDetection`
   (`crates/lctrl-hal/src/conflict.rs:4-8`, `fn active_vendor_controllers(&self) ->
   Result<Vec<String>>`) returning one name, attached through
   `CommandServices::with_conflict_detection` (`lib.rs:221`), with a recording fake backend of the
   `MachineFixControl` kind. The gate is `apply == Commit && command_is_mutating(..)`
   (`lib.rs:1860`) before `preflight_control_conflicts` (`lib.rs:2295`, `None` short-circuit at
   `:2299-2301`), so the three assertions are:
   - `vantage --dry-run fix run x` → `Ok`, detector call-count **0** (the preflight is never
     consulted on a dry run), fake mode log `[DryRun]`;
   - `vantage fix run x` without `--yes` (interactive) → `Err(InvalidArgument)`, detector called
     once, fake mode log **empty** (no write reached the service);
   - `vantage --yes fix run x` → `Ok`, fake mode log `[Commit]` — this is the leg that proves the
     new F8 arm is load-bearing rather than merely present.
   Repeat the pair for one read-only neighbour (`vantage fix scan`) to show the detector is not
   consulted there either.
6. **Contract tests** (green before/after; this item changed no declarations):
   `cargo test -p vantage-cli-lib --test cli_contract`. **Package name matters**: the lib and its
   `tests/` live in `crates/vantage-cli/Cargo.toml` as package `vantage-cli-lib`; `vantage-cli` is
   the *bin* crate at `vantage/Cargo.toml`, so `-p vantage-cli` runs no such test target (this also
   corrects `re-cli-surface-20261105.md` §8.6, which spells the package wrong). `wave6-1` is adding
   `crates/vantage-cli/tests/dry_run_resolution.rs` for the mode-resolution regression — do not
   duplicate it in `cli_contract.rs`. The existing precedent for the shape is
   `global_dry_run_reaches_performance_service_without_commit`
   (`crates/vantage-cli/tests/cli_contract.rs`), i.e. a recording stub backend asserting
   `mode == DryRun` and zero write calls.
6. **Build risk to resolve before any merge gate**: three workers edited the same file
   (`lib.rs`) plus `lctrl-hal{,-win,-linux}/src/radio.rs` without a build. Run
   `cargo check -p vantage-cli --target x86_64-pc-windows-msvc` (and the Linux target) before
   trusting any of §5.

## 7. Open items / unproven

- F6 `wifi clear-history` and F7 `daemon stop` remain unpatched by instruction (owners decide
  semantic: refuse vs. no-op-with-plan vs. out of scope).
- Nothing here is verified behaviour: no build, no test, no target run. §6 is the minimum live
  evidence.
- `crates/vantage-cli/src/lib.rs` was under concurrent edit for the whole audit; if a later hunk
  contradicts §5, the later writer wins and this document is stale for that row.

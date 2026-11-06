# Capability registry consistency audit — 2026-11-06

Source-only audit of the static command-capability registry after a day of concurrent
worker patches. Every statement below is derived by parsing the tree at the pinned snapshot;
**no hardware was probed, no `vantage capability` output is claimed, and no source file was
modified by this audit.**

## Snapshot (pins the bytes this audit read)

| file | bytes | sha256 |
|---|---|---|
| `crates/lctrl-core/src/command_capability.rs` | 30158 | `9075ddd0cdf425dc381f88f2de5727ddb1ae6c653d10bbdc5add6b55e9dcc761` |
| `crates/lctrl-hal/src/command_capability.rs` | 87634 | `d3e6c1f46f9974d2a32a784af78debd556e5b88aaa22a15dcfa51aeee55b76ca` |
| `crates/vantage-cli/src/capability.rs` | 48937 | `bec6ec61c8a400f12d346d8cb6f21dd9656263c1056bf7ef1d61994abe7ae944` |

Audited 2026-11-06 12:19 (UTC+8). The worktree is shared, dirty, and being edited by ~27 live workers;
the registry files changed several times *during* this audit (ids 155 → 156, `Capability`
bindings 106 → 113 → 93 distinct ids). Re-run the checks if these hashes no longer match.
Snapshot counts: **156 `CommandId` consts**, **156 `CommandId::ALL`
entries**, **159 `COMMAND_REGISTRY` rows** (`crates/lctrl-hal/src/command_capability.rs`
L253–L2264).

## Results

| # | check | result |
|---|---|---|
| 1 | every `CommandId` const has one `ALL` entry and ≥1 registry row | **1 gap** — `CommandId::DISPLAY_STATUS` has no row |
| 2 | every `FeatureBinding::Capability(id)` id is recorded by a live probe | **4 ids with no recorder** (all four rows are `DispatchSupport::Unsupported`); plus **4 `Implemented` rows permanently `Unverified`** because their `Unprobed` id has no probe (§2e) |
| 3 | every used `ServiceRole` has an `as_str` arm and a `role_channel` mapping | `as_str` 36/36; `role_channel` **2 gaps** (`MagicBayCamera`, `MagicBayDisplay`) |
| 4 | every `Dependency` variant handled by `platform()` and `component()` | **PASS** 49/49 arms, both exhaustive |
| 5 | no two rows share one command identity | **PASS** 0 duplicate `(CommandId, StateLayer)` |
| 6 | `cargo check -p lctrl-core -p lctrl-hal` | **PASS** host + `x86_64-pc-windows-msvc` |

No fix was applied: check 1's gap needs the command owner's dispatch/probe facts (inventing
them is exactly what this audit is forbidden to do), and the check 2/3 items are metadata
claims that change no observed status (see severity notes). Exact proposals are in
“Recommended bounded changes”.

---

## 1. `CommandId` ↔ `CommandId::ALL` ↔ `COMMAND_REGISTRY`

| check | count | verdict |
|---|---|---|
| `pub const` ids in `crates/lctrl-core/src/command_capability.rs` | 156 | — |
| duplicate const names / duplicate id strings | 0 / 0 | PASS |
| entries in `CommandId::ALL` | 156 | — |
| duplicate `ALL` entries | 0 | PASS |
| consts missing from `ALL` | 0 | PASS |
| `ALL` entries with no const | 0 | PASS |
| registry rows | 159 | — |
| **consts with no registry row** | **1** | **GAP: `DISPLAY_STATUS`** |
| rows whose id has no const | 0 | PASS |

### 1a. The one missing row

- `CommandId::DISPLAY_STATUS` = `"display.status"`, declared at
  `crates/lctrl-core/src/command_capability.rs:90` and listed in `CommandId::ALL` at
  `crates/lctrl-core/src/command_capability.rs:270`; `COMMAND_REGISTRY` contains no row for it

Whole-tree evidence that the id is in flight and unconsumed:
```
$ grep -rn DISPLAY_STATUS crates/ --include=*.rs
crates/lctrl-core/src/command_capability.rs:90:    pub const DISPLAY_STATUS: Self = Self("display.status");
crates/lctrl-core/src/command_capability.rs:270:        Self::DISPLAY_STATUS,
```

Observable consequences:
- `crates/vantage-cli/tests/cli_capability.rs::registry_covers_every_command_id` iterates
  `CommandId::ALL` and asserts `!command_capabilities(*id).is_empty()` → this test is red on
  the current tree purely because of this id.
- `lctrl_hal::command_capability_for(CommandSelector::new(DISPLAY_STATUS, layer))` returns `None`.
- No `vantage-cli` parse arm maps any `Command` to `DISPLAY_STATUS` yet (no `CommandId::`
  reference outside `lctrl-core`), so the id is currently unreachable from the CLI as well.

Who can close it: the owner of `display status`. The row needs real axes
(read-only / `Runtime`), a real `FeatureBinding`, a `ServiceRole`, and the dependency list of
the actual probe. This audit deliberately does not invent any of them.

### 1b. Legitimate multi-row commands (layer selectors)

| command | rows (`CommandCapability::new` line, layer, dispatch) |
|---|---|
| `PRIVACY_CAM` | L1166(`Runtime`, Implemented), L1181(`BiosPersistent`, Implemented) |
| `PRIVACY_MIC` | L1193(`Runtime`, Implemented), L1209(`BiosPersistent`, Implemented) |
| `USB_ALWAYS_ON` | L404(`Runtime`, Implemented), L416(`BiosPersistent`, Implemented) |
| `USB_CHARGE_ON_BATTERY` | L428(`Runtime`, Implemented), L440(`BiosPersistent`, Implemented) |

All other ids have exactly one row. These four are distinct `(CommandId, StateLayer)`
selectors, which is the registry's identity (`CommandSelector { command, layer }`).

---

## 2. `FeatureBinding::Capability(id)` — is the id recorded by a live probe?

Distinct ids bound as `Capability`: **93** (across 113 rows).
Derivation: for every `crates/lctrl-hal-win/src/*.rs` and `crates/lctrl-hal-linux/src/**/*.rs`
file (tests excluded) the audit collects (a) the first argument of every `.record(name, …)`
call, (b) every string literal in a `for … in [ … ]` batch whose body calls `.record`, and
(c) string literals passed to record helpers (`record_node`, `record_fixed_unsupported`,
`record_event_log_capability`, `record_fn_osd`, `record_hotkey_status`). No `record(format!(…))`
or otherwise computed feature id exists in either HAL (grep: 0 hits), so the corpus is complete
for statically named ids: **146 names on Windows, 40 on Linux**.

### 2a. Ids with **no recorder at all** (4)

| id | row | layer | row dispatch | reader renders | row at | where else the literal appears |
|---|---|---|---|---|---|---|
| `battery.extreme-life` | `BATTERY_EXTREME_LIFE` | `Runtime` | `Unsupported` | `Unsupported` | L358 | **nowhere** |
| `panel.eye-care.write` | `PANEL_EYE_CARE` | `Runtime` | `Unsupported` | `Unsupported` | L1149 | **nowhere** |
| `panel.super-resolution.write` | `PANEL_SUPER_RESOLUTION` | `Runtime` | `Unsupported` | `Unsupported` | L1118 | **nowhere** |
| `perf.fan.curve` | `PERF_FAN_CURVE` | `Runtime` | `Unsupported` | `Unsupported` | L658 | `crates/lctrl-hal-win/src/thermal.rs:342`, `crates/lctrl-hal-linux/src/lib.rs:1147` |

Every other occurrence is an error *label*, never a probe result:
- `perf.fan.curve` — the `LctrlError::Unsupported { feature }` string in
  `crates/lctrl-hal-win/src/thermal.rs:342` and `crates/lctrl-hal-linux/src/lib.rs:1147`
  (`fn fan_table(&self, …)` has no writer on either platform).
- `panel.super-resolution.write` / `panel.eye-care.write` — CLI refusal labels
  (`feature: "panel.…write"`), not capability records; the panel writers live in
  `crates/lctrl-hal-win/src/panel_features.rs` and record no such id.
- `battery.extreme-life` — appears only in the registry itself plus the row's `notes`, which
  already explains that AiTurbo extreme-life is a run-state policy with no firmware selector.

**Severity (important): none of these four renders `Unverified` because of the missing probe.**
All four rows carry `DispatchSupport::Unsupported(…)`, and `evaluate_command`
(`crates/vantage-cli/src/capability.rs`) resolves `(DispatchSupport::Unsupported, ..)` to
`ObservedStatus::Unsupported` *before* looking at `FeaturePresence`. The defect is a false
claim in the registry: `Capability(…)` asserts "a probe records this"; `Unprobed(…)` is the
variant the module documents for "meaningful feature name, but no current capability probe".

### 2b. Inverse case — `Unprobed(id)` ids a live probe *does* record (2)

| id | recorded at | rows bound `Unprobed` |
|---|---|---|
| `battery.thresholds` | `crates/lctrl-hal-linux/src/lib.rs:847`, `crates/lctrl-hal-win/src/windows_hal.rs:292` | `BATTERY_THRESHOLDS` |
| `magicbay.lte` | `crates/lctrl-hal-win/src/windows_hal.rs:515` | `MAGICBAY_LTE_APN`, `MAGICBAY_LTE_CONNECT`, `MAGICBAY_LTE_DISCONNECT` |

Runtime status is unaffected for the same reason as 2a: `feature_presence` resolves
`Unprobed(feature)` through the same `capability_presence(feature, capabilities)` lookup as
`Capability(feature)`. Only the registry's own claim is inconsistent.

### 2c. `AnyOf` / `Dynamic` feature ids (18 distinct)

All 18 ids are recorded by a Windows probe. No gap.

### 2d. `Unprobed` ids with no probe at all (17 distinct) — honest by design

`bios.password.clear`, `bios.password.set`, `bios.save.global-buffer`, `magicbay.cam`, `magicbay.display`, `panel.color.write`, `panel.overdrive.write`, `radio.off`, `radio.on`, `sense.kbd-light-auto`, `update.download.without-authenticated-manifest`, `update.history.ledger`, `update.ignore.without-catalog`, `update.install.without-authenticated-manifest`, `update.local_package`, `update.rollback.without-install-ledger`, `update.schedule.without-catalog`

(The other two `Unprobed` ids — `battery.thresholds`, `magicbay.lte` — are recorded live; see §2b.)

13 of these 17 back `DispatchSupport::Unsupported` rows and render `Unsupported`. The remaining
four back `Implemented` rows and therefore render `Unverified` — see §2e.

### 2e. `Implemented` rows whose binding admits no probe (permanently `Unverified`)

Not a check-2 violation (the bindings say `Unprobed` honestly), but the same user-visible effect
the check is looking for: these four commands are dispatchable yet can never reach `Available`.

| command | layer | feature binding | service role | row at | status reasoning |
|---|---|---|---|---|---|
| `RADIO_OFF` | `Runtime` | `Unprobed("radio.off")` | `Radio` (`channel.radio` recorded) | L2064 | service present, feature `Unverified` → `Unverified` |
| `RADIO_ON` | `Runtime` | `Unprobed("radio.on")` | `Radio` (`channel.radio` recorded) | L2078 | same |
| `UPDATE_LOCAL_PACKAGE` | `NotApplicable` | `Unprobed("update.local_package")` | `Update` (`channel.update-local` recorded) | L1822 | same |
| `UPDATE_HISTORY` | `LocalPersistent` | `Unprobed("update.history.ledger")` | `Update` (`channel.update-local` recorded) | L1918 | same |

`evaluate_command` only reaches `Available` through `FeaturePresence::Available`, so no amount of
live channel presence lifts these rows. Closing it needs either a recorder for those exact ids or
a rebinding to a recorded id (`radio.status` / `channel.radio`, `update.local_driver`, …) — a
semantic choice for the owner of each command, which this audit does not make.

---

## 3. `ServiceRole` — `as_str` and `role_channel` coverage

- variants in `ServiceRole`: **36**
- `as_str` arms: **36**, exhaustive `match self` with **no wildcard arm** → compiler-enforced, 0 gaps
- `role_channel` (`crates/vantage-cli/src/capability.rs`, function at L1004): **33 explicit arms + `_ => None` wildcard** (a wildcard arm is present, so a new role silently maps to `None`)
- `Hal` is deliberately absent from `role_channel`: `role_present` returns `Present` for it first

| role | used by rows | `as_str` | `role_channel` | that channel recorded by a probe |
|---|---|---|---|---|
| `Hal` | 4 | `hal` | `None` (special-cased `Present`) | n/a |
| `Battery` | 14 | `battery` | `channel.battery` | yes |
| `Bios` | 19 | `bios` | `channel.bios` | yes |
| `Diagnostics` | 2 | `diagnostics` | `channel.diagnostics` | yes |
| `Fan` | 8 | `fan` | `channel.fan` | yes |
| `Keyboard` | 3 | `keyboard` | `channel.keyboard` | yes |
| `MagicBay` | 1 | `magicbay` | `channel.magicbay` | yes |
| `Performance` | 8 | `performance` | `channel.performance` | yes |
| `Power` | 7 | `power` | `channel.power` | yes |
| `Privacy` | 3 | `privacy` | `channel.privacy-runtime` | yes |
| `Touchpad` | 4 | `touchpad` | `channel.touchpad` | yes |
| `Panel` | 4 | `panel` | `channel.panel` | yes |
| `PanelBrightness` | 2 | `panel_brightness` | `channel.panel-brightness` | yes |
| `PanelIdentity` | 1 | `panel_identity` | `channel.panel-identity` | yes |
| `Temperature` | 5 | `temperature` | `channel.temperature` | yes |
| `Tuning` | 8 | `tuning` | `channel.tuning` | yes |
| `Update` | 13 | `update` | `channel.update-local` | yes |
| `Events` | 2 | `events` | `channel.events` | yes |
| `Process` | 3 | `process` | `channel.process` | yes |
| `ProcessorPower` | 1 | `processor_power` | `channel.processor-power` | yes |
| `Usb` | 3 | `usb` | `channel.usb` | yes |
| `Sense` | 10 | `sense` | `channel.hpd-rpc` | yes |
| `Audio` | 5 | `audio` | `channel.audio` | yes |
| `MobileBroadband` | 4 | `mobile_broadband` | `channel.mobilebroadband` | yes |
| `MagicBayCamera` | 1 | `magicbay_camera` | **missing** | no such probe exists anywhere |
| `MagicBayDisplay` | 1 | `magicbay_display` | **missing** | no such probe exists anywhere |
| `Osd` | 4 | `osd` | `channel.osd` | yes |
| `Storage` | 1 | `storage` | `channel.storage` | yes |
| `Hotkey` | 5 | `hotkey` | `channel.hotkey` | yes |
| `Wifi` | 2 | `wifi` | `channel.wifi` | yes |
| `Bluetooth` | 1 | `bluetooth` | `channel.bluetooth` | yes |
| `Radio` | 3 | `radio` | `channel.radio` | yes |
| `Sensor` | 2 | `sensor` | `channel.sensor` | yes |
| `Security` | 1 | `security` | `channel.security` | yes |
| `MachineFix` | 3 | `machine_fix` | `channel.machinefix` | yes |
| `ScenePolicy` | 2 | `scene_policy` | `channel.scene` | yes |

Gaps:
- `MagicBayCamera` — used by `MAGICBAY_CAM` (L1639), whose dispatch is `Unsupported("standard UVC camera control is not attached")`.
- `MagicBayDisplay` — used by `MAGICBAY_DISPLAY` (L1650), dispatch `Unsupported("standard display control is not attached")`.
- Neither role has a `channel.*` probe on any platform, so `None` is the honest mapping today;
  both rows resolve to `Unsupported` before service presence is consulted.

Latent defect, not a live one: the `_ => None` wildcard means a *future* role silently maps to
`None` and its rows can only ever be `Unverified`. `as_str` shows the alternative (no wildcard,
compiler-enforced). Recorded `channel.*` names that no role maps (dependency-level transports,
informational): `channel.dolby`, `channel.elevoc`, `channel.energy_driver`, `channel.fn-osd`, `channel.mmdevice`, `channel.wmi.root`.

---

## 4. `Dependency` — `platform()` and `component()`

| check | result |
|---|---|
| `Dependency` variants | 49 |
| variants in `platform()` | 49 — all variants, **no wildcard arm** |
| variants in `component()` | 49 — all variants, **no wildcard arm** |
| variants missing from `platform()` | 0 |
| variants missing from `component()` | 0 |
| names in `platform()` that are not variants | 0 |
| names in `component()` that are not variants | 0 |
| dependency names used by registry rows that are not variants | 0 |

Both are exhaustive `match self` over the same enum with no `_ => …` escape, so the compiler
enforces check 4 for every future variant. `capability_id()` (2 arms + wildcard) and
`switch_guidance()` (13 arms + wildcard) are deliberately partial and out of scope here.

---

## 5. Row identity

- duplicate `(CommandId, StateLayer)` selector pairs: **0**
- duplicate `CommandId` in `ALL`: 0; duplicate id strings among consts: 0; duplicate const names: 0

The four two-layer commands in §1b are the only repeats and each pair differs by `StateLayer`.

**Guard weakness found while checking this** (test, not production code):
`crates/vantage-cli/tests/cli_capability.rs::registry_covers_every_command_id` ends with

```rust
assert_eq!(
    command_registry().iter()
        .map(|entry| (entry.command, entry.axes.layer))
        .collect::<Vec<_>>().len(),
    command_registry().len()
);
```
`collect::<Vec<_>>().len()` equals `command_registry().len()` by construction, so the assertion
is a tautology and can never catch a duplicated selector. A working guard collects the pairs
and compares against a de-duplicated set (`HashSet` or sort+dedup).

---

## 6. Compile state (`lctrl-core`, `lctrl-hal`)

```
$ cargo check -p lctrl-core -p lctrl-hal --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.15s

$ cargo check -p lctrl-core -p lctrl-hal --offline --target x86_64-pc-windows-msvc
    Checking lctrl-core v0.1.0 (crates/lctrl-core)
    Checking lctrl-hal v0.1.0 (crates/lctrl-hal)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.86s
```
Run 2026-11-06T04:20:02Z against the live tree: both crates compile for the host and for the
Windows target. Known in-flight breakage reported by other workers lives in `lctrl-hal-win`
(`usb_ports.rs`, `radio.rs`, `event_log.rs`), which neither command builds.

---

## Recommended bounded changes (none applied)

1. **`DisplayStatus` row** — the owner of `display status` adds the `CommandCapability::new(...)`
   row (capability id, `ServiceRole`, dependency list) to `COMMAND_REGISTRY`. Until then
   `cli_capability::registry_covers_every_command_id` is red. Do not copy another command's
   probe: the id has no recorder and no CLI parse arm yet.
2. **Four `Capability` → `Unprobed` reclassifications** in
   `crates/lctrl-hal/src/command_capability.rs`: L358 `BATTERY_EXTREME_LIFE`, L1149 `PANEL_EYE_CARE`, L1118 `PANEL_SUPER_RESOLUTION`, L658 `PERF_FAN_CURVE`.
   `FeatureBinding::Unprobed(..)` keeps the id string and the observed status (both variants
   consult the live set) while dropping the false "a probe records this" claim.
   `is_probed()` / `direct_feature()` have no consumer in the workspace (grep: 0 hits) and
   `ObservedCommandCapability::feature_ids` is empty for both variants, so no output changes.
   Bounded to four one-line edits, not made: that file is under active concurrent edit and the
   same rows belong to other workers' Issues.
3. **Two `Unprobed` → `Capability` promotions** (`battery.thresholds`, `magicbay.lte`) once the
   owner confirms the Windows probe is the intended authority — same no-op semantics, opposite
   direction of claim.
4. **Make `role_channel` exhaustive** — replace `_ => None` with an explicit
   `ServiceRole::Hal | ServiceRole::MagicBayCamera | ServiceRole::MagicBayDisplay => None`, so a
   future role cannot silently lose its channel (two lines in `crates/vantage-cli/src/capability.rs`, function at L1004;
   a shared CLI surface, so its owner should take it).
5. **Fix the tautological duplicate-selector assertion** in
   `crates/vantage-cli/tests/cli_capability.rs`.
6. **Unstick the four permanently-`Unverified` commands** (§2e): `RADIO_ON`, `RADIO_OFF`,
   `UPDATE_LOCAL_PACKAGE`, `UPDATE_HISTORY`. Each needs either a recorder for its exact feature
   id or a rebinding to an id the Windows probe already records (`radio.status`/`channel.radio`,
   `update.local_driver`). Which one is correct is the command owner's decision; binding a
   channel id as a feature would be wrong on its face, so this audit states the options only.
   Note `crates/vantage-cli/tests/cli_capability.rs` already pins several statuses, so whichever
   choice is made has a test to update.

---

## Not verified (parent-owned)

- No test was run. Acceptance commands: `cargo test -p lctrl-hal`, `cargo test -p lctrl-core`,
  `cargo test -p vantage-cli --test cli_capability` (the last is expected red on `DisplayStatus`
  at this snapshot, §1a).
- No `vantage capability` run on 21VG. "The four §2a rows render `unsupported`" is a read of
  `evaluate_command`'s match order, not an observation. Meaningful check: `vantage capability
  --json` on the target and confirm those four are `unsupported`, with the negative control
  that none of them is `available`; a row flipping elsewhere would mean something records it.
- Linux parity, measured but out of scope: **72 of 93** `Capability` ids have no `lctrl-hal-linux` recorder, so
  on Linux they can only render `unverified`. Consistent with a Linux HAL that implements fewer
  channels, not a registry error — never "fixed" by adding ids.
- Recorder *liveness* was not re-verified: the audit proves an id is recorded in the platform's
  HAL, not that the probe succeeds on hardware.

## Method (reproducible)

1. Strip `//` and `/* */` comments from the three registry files and the recorder corpus (string
   literals preserved, Rust `\`+newline continuations kept).
2. Split `COMMAND_REGISTRY` on balanced `CommandCapability::new(` calls, then split each argument
   list at top-level commas → 6 fields per row (`command, axes, feature, service, deps, dispatch`).
3. Collect recorded ids as in §2; diff against `Capability` bindings.
4. Count `ServiceRole` variants/arms and `Dependency` variants plus both match bodies.
5. Gotcha found during the audit: a string regex without `DOTALL` desynchronises on `\`+newline
   inside the long `detail` literals and silently drops later ids; all counts above use `DOTALL`.

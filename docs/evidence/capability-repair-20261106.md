# Capability-matrix repair — role joins, blind rows, `update check` — 2026-11-06

Follow-up to `docs/evidence/capability-audit-20261105.md` (§4 A1, §5.2, §6.2). This note repairs the
two structural defects that made the matrix misreport **working** commands, plus the one
`Unsupported` row whose channel is live.

Boundary of this change: no dispatch *verb* was rewritten and no command's execution path changed —
the only `DispatchSupport` edit is the `UPDATE_CHECK` static claim (§3), which is report-only
(§3 shows the registry has no runtime consumer outside the capability evaluator). No
axes/dependency/hardware column was touched, no write was issued, no service was changed, no target
command was run by this worker. The compiler/IR is another owner's surface and is untouched.

The tree is shared and dirty by design (~30 live workers). Sibling workers landed recorders **while**
this repair was being written; §0 separates what they landed from what this change added, so the
diff can be attributed per id.

## 0. Tree state at the time of this repair

Already landed by peers (verified here, **not** changed by this repair):

| id | recorder | row binding |
|---|---|---|
| `events.log` + `channel.events` | `lctrl-hal-win/src/event_log.rs` (`record_event_log_capability`, called at `windows_hal.rs`) | `EVENTS_LOG` = `Capability` |
| `channel.storage` + `storage.status` | `windows_hal.rs` (`probe_storage`) | `STORAGE_STATUS` = `Capability` |
| `channel.sensor` + `sensor.list` | `windows_hal.rs` (`SensorControl::list`) | `SENSOR_LIST`/`SENSOR_READ` = `Capability` |
| `channel.bluetooth` + `bluetooth.list` | `windows_hal.rs` (`WindowsBluetooth::bluetooth_devices`) | role mapped; row still `Unprobed` (§2) |
| `channel.security` + `security.status` | `windows_hal.rs` (`observe_security`) | role mapped (`SECURITY_STATUS`) |
| `usb.ports` | `lctrl-hal-win/src/usb_ports.rs` (`record_usb_port_capabilities`) | `USB_PORTS` = `Capability` |
| `wifi.status` | `windows_hal.rs` (`WindowsWifiStatus::wifi_status`) | row still `Unprobed` (§2) |
| role `Events` | `crates/vantage-cli/src/capability.rs::role_channel` | done by peer |

Added by this repair: `channel.keyboard` (Windows recorder), `channel.radio` + `radio.status`,
`update.catalog.local`, role arms `Keyboard`/`Radio`/`Sensor`/`Storage`, the exhaustive arm list in
`role_channel`, and the `UPDATE_CHECK` rebind.

**Collision resolved, flagged for the record:** a peer added the same
`ServiceRole::Storage => Some("channel.storage")` arm while this repair was being applied; the file
briefly held two identical arms. The duplicate was removed rather than merged, so one arm per role
survives (a duplicated arm is an `unreachable_patterns` warning, i.e. CI-breaking under
`-D warnings`).

## 1. Defect A — `ServiceRole` variants with no `role_channel()` entry (audit §6.2)

`role_present()` maps a role through `role_channel()`; an unmapped role is `Unexposed`, and
`evaluate_command()` turns `Unexposed` on an `Implemented` row into `Unverified` **whatever the
feature probe says**. A row bound to such a role therefore could never read `available`.

`crates/vantage-cli/src/capability.rs::role_channel` now maps every role that a registry row uses:

| role | channel id | recorder that must have written it | rows affected | status before → after |
|---|---|---|---|---|
| `Keyboard` | `channel.keyboard` | **added**: `windows_hal.rs:346` (`for feature in ["kbd.backlight", "channel.keyboard"]`, fed by the existing `probe_vpc_keyboard` result). Linux already recorded the same id (`lctrl-hal-linux/src/lib.rs:873`) | `KBD_BACKLIGHT`; the `Keyboard` leg of the `TUNE_PROFILE_APPLY`/`TUNE_RESTORE` composites | `unverified` → `available` (KBLC answers: `out=0x7`, group 3, states 0..=3) |
| `Events` | `channel.events` | `event_log.rs` (peer) | `EVENTS_LOG` | `unverified` → follows the `events.log` probe |
| `Storage` | `channel.storage` | `windows_hal.rs` storage probe (peer) | `STORAGE_STATUS` | `unverified` → follows `storage.status` |
| `Sensor` | `channel.sensor` | `windows_hal.rs` sensor probe (peer) | `SENSOR_LIST`, `SENSOR_READ` | `unverified` → follows `sensor.list` |
| `Radio` | `channel.radio` | **added**: `windows_hal.rs:1275` (`RadioControl::radio_status(&NativeRadio::new())`) | `RADIO_STATUS`, `RADIO_OFF`, `RADIO_ON` | `RADIO_STATUS`: `unverified` → `available`; `RADIO_OFF`/`RADIO_ON`: `unverified` → `unverified` (no feature probe, §2) |
| `Bluetooth` | `channel.bluetooth` | `windows_hal.rs` (peer) | `BLUETOOTH_LIST` | `unverified` → follows `bluetooth.list` |
| `Security` | `channel.security` | `windows_hal.rs` (peer) | `SECURITY_STATUS` | `unverified` → follows `security.status` |
| `MagicBayCamera`, `MagicBayDisplay` | **none, deliberately** | no recorder exists, and the camera/display control planes are absent on this SKU (`re-magicbay-20261105.md` §3) | both rows are `Unsupported` on this SKU, so the role is moot | unchanged (`unverified` cannot be reached: `DispatchSupport::Unsupported` returns first) |

`kbd backlight` is the case that made the defect visible: the channel has been live since the KBLC
recovery (`re-kbd-backlight-20261105.md` §3) — `\\.\EnergyDrv` IOCTL `0x83102144`, `KBLC(0x1)` →
`0x7` — while the matrix said `unverified` because `ServiceRole::Keyboard` had no channel.

**Structural guard added.** The old `_ => None` catch-all is replaced by explicit
`ServiceRole::Hal | ServiceRole::MagicBayCamera | ServiceRole::MagicBayDisplay => None`
(`capability.rs:1060-1062`). The wildcard is what let this defect exist silently: a new variant
inherited `Unexposed` with no compiler signal. Now a new `ServiceRole` variant fails to compile
until its channel (or its deliberate absence) is decided.

Cross-check run at the end of this change: every id returned by `role_channel` has at least one
recording site in `crates/lctrl-hal-win/src` or `crates/lctrl-hal-linux/src` — no mapping points at
an id nothing writes.

## 2. Defect B — `Implemented` rows bound to an id nothing records (audit §5.2)

| row | id | recorder at the end of this change | binding before → after |
|---|---|---|---|
| `EVENTS_LOG` | `events.log` | peer (`event_log.rs`) | `Capability` (already) |
| `USB_PORTS` | `usb.ports` | peer (`usb_ports.rs`) | `Capability` (already) |
| `STORAGE_STATUS` | `storage.status` | peer (`windows_hal.rs`) | `Capability` (already) |
| `WIFI_STATUS` | `wifi.status` | peer (`windows_hal.rs`, WLAN link-state query) | `Unprobed` → **`Capability`** |
| `BLUETOOTH_LIST` | `bluetooth.list` | peer (`windows_hal.rs`, non-inquiring enumeration) | `Unprobed` → **`Capability`** |
| `RADIO_STATUS` | `radio.status` | **this change** (`windows_hal.rs:1275`) | `Unprobed` → **`Capability`** |
| `RADIO_OFF`, `RADIO_ON` | `radio.off`, `radio.on` | **none, deliberately** | `Unprobed` (unchanged) |
| `UPDATE_LOCAL_PACKAGE` | `update.local_package` | **none, deliberately** | `Unprobed` (unchanged) |
| `UPDATE_HISTORY` | `update.history.ledger` | **none, deliberately** | `Unprobed` (unchanged) |

Why the last three were not given a recorder:

* `radio.off`/`radio.on` are mutations with `Readback::Required` + `Rollback::PreviousState`. The
  only cheap read-only probe is the status enumeration, which proves the *read* channel, not an
  owned set-state. Recording them from it would be the over-claim the same audit rejects for the
  nine `sense.*` rows (§6.5). They stay `unverified` until an executed `radio off --yes` acceptance
  (with readback and rollback observed) exists to justify the id.
* `update.local_package` (`update local-install <dir>`) has no probe-able precondition: its gate is
  an operator-supplied package directory plus the package's own signature. A capability probe cannot
  name that directory, and inventing an `available` claim from "the gate code exists" is not a
  channel observation.
* `update.history.ledger`: the honest probe is `update_history::read_history()`, which runs
  `SELECT … FROM updatehistory` over **both** ledger databases with no `LIMIT`
  (`lctrl-hal-win/src/update_history.rs`, `INSTALL_EVENTS_SQL`/`PACKAGE_STATUS_SQL`). That is a full
  row enumeration inside `capabilities()`, so it violates the "cheap, no full enumeration" rule for
  a probe. The bounded repair is a precondition probe — ledger file exists and the engine opens —
  and belongs to the update-family owner, not to a matrix repair.

The three flips are **behaviour-neutral by construction**: `feature_presence()` evaluates
`Unprobed(feature)` and `Capability(feature)` through the same `capability_presence()` call
(`crates/vantage-cli/src/capability.rs:1129-1130`), and `feature_ids` prints the id for both
variants. Only the static claim "no current capability probe" was false and is now gone.

## 3. Defect C — `UPDATE_CHECK` was `Unsupported` while the verb works (audit §4 A1)

Before:

```rust
FeatureBinding::Unprobed("update.capability"),
DispatchSupport::Unsupported("remote update checking has no authenticated catalog contract"),
```

`vantage update check` serves the catalogue the UDC service already synced
(`HKLM\SOFTWARE\Lenovo\UDC\Packages\LastUpdateConfig`): `UpdateCommand::Check` prefers
`UpdateControl::check_updates()` and only falls back to the capability summary
(`crates/vantage-cli/src/lib.rs:7158` arm). The live run recorded in the audit (VP-30) returned
exit 0 with `last_sync 2026-10-05T20:02:25+08:00` and three packages. The matrix told users a working
read command was broken.

After (`crates/lctrl-hal/src/command_capability.rs:1780-1806`):

```rust
FeatureBinding::Capability("update.catalog.local"),
DispatchSupport::Implemented,
```

with the new read-only probe:

* `lctrl-hal-win/src/update_catalog.rs` — `pub struct CatalogSync { last_sync, packages, stale }`
  and `pub fn catalog_sync() -> Result<Option<CatalogSync>>` (Windows impl at `:440`, non-Windows
  `Unsupported` stub at `:530`). It reuses the existing private `read_config()` (one `RegGetValueW`
  for `Packages\LastUpdateConfig` + JSON parse) plus the `UpdateCheckDateTime` sibling value and
  **stops before** `evaluate()`: no network, no package applicability, no install, no write.
* `lctrl-hal-win/src/windows_hal.rs:1538-1583` records `update.catalog.local`:
  `available` when a parseable config lists ≥1 package and is not marked update-required,
  `limited` when the service marked the cached config stale (`ConfigUpdateRequired=1`) or the synced
  config lists no packages, `unavailable` when no `LastUpdateConfig` exists or it does not parse —
  with the parse error as the detail. A missing catalogue is **not** a failure of the verb (it falls
  back to the capability summary), so `unavailable` is the honest record and the row follows it.
* `lctrl-hal-linux/src/lib.rs:721-736` records the same id as `unavailable` with the Windows-only
  boundary, so the row reads `unavailable` on Linux instead of `unverified`.

Both recorders name the split in the detail text: `"local synced catalogue: last sync …, N
package(s) listed; remote refresh requires an authenticated UDC contract"`.

**The remote-refresh refusal is preserved verbatim** —
`"remote update checking has no authenticated catalog contract"` — in the row comment
(`command_capability.rs:1783-1797`) and here. It is also still machine-readable elsewhere:

* `UPDATE_DOWNLOAD`/`UPDATE_INSTALL`/`UPDATE_SCHEDULE` are `Unsupported` with the
  `update.*.without-authenticated-manifest`/`without-catalog` ids;
* `vantage doctor`'s `update.capability` channel keeps reporting FAIL with the remote-contract
  reason (`crates/vantage-cli/src/lib.rs:2539-2556`) — the doctor check is a channel name, not a
  capability record, so it is unaffected by the rebind. This is the §6.4 split: the two surfaces now
  name different legs instead of contradicting each other.

Where the distinction is visible to a user:

* `vantage capability --json` — the row now reads `available`/`limited` with
  `feature_ids: ["update.catalog.local"]`;
* `vantage info --json` — `features["update.catalog.local"].detail` carries the sentence above
  (`InfoPayload.features`, `crates/vantage-cli/src/lib.rs:1769-1776`);
* `vantage doctor` — still the remote leg.

**Report-only, verified:** `DispatchSupport` is consumed only by the capability evaluator — the
sole matches on it in the workspace are in `crates/vantage-cli/src/capability.rs`
(`evaluate_command`, plus the `Bios` staging-block check). No dispatcher consults the registry, so a
row changing from `Unsupported` to `Implemented` cannot change what a verb does; it changes what the
matrix says the verb does.

Rejected alternatives: (a) keep `Unsupported` and re-word it — leaves the working verb reported
broken; (b) flip to `Implemented` while leaving `Unprobed("update.capability")` — a blind row, the
defect §5.2 describes; (c) split into a second registry row for the remote refresh — that row would
have no verb, and the refusal is already carried by the `update download/install/schedule` rows.

## 4. Deliberately not changed

* `DispatchSupport` of every row except `UPDATE_CHECK`.
* Axes, dependencies, hardware requirements, `notes`, `snapshot_*` feature/service lists.
* The nine `sense.*` ids (§6.5 over-claim, separate owner decision).
* `command_selector`/`command_id` over-permissiveness for bare `power` (audit §6.3, cosmetic).
* The 19+ `update.*.without-…`/`bios.save.global-buffer`/`panel.*.write` unprobed ids — they describe
  a *refused* contract, not a channel; recording them would invert their meaning.
* `docs/evidence/target-capability-matrix-20261105.json` and every published matrix: they are
  snapshots of the 2026-11-05 14:23 binary and must be regenerated from a build of this tree
  (§6).
* `crates/lctrl-hal-win/src/radio.rs` (another worker's in-flight module): this repair only *calls*
  its read-only status path.
* Linux `channel.update-local` is still unrecorded, so `ServiceRole::Update` is `Unexposed` on Linux
  and the other `update.*` rows read `unverified` there. Pre-existing, outside this repair, recorded
  here so it is not mistaken for a new regression.

## 5. Verification status of this note (honest limits)

* This worker ran **no** build, test, lint, formatter or target command (worker role: the parent owns
  verification; the tree also carries peers' in-flight edits in `usb_ports.rs`, `radio.rs`,
  `event_log.rs`, `storage.rs`, so a failure would not be attributable to this change). Every
  `before → after` above is **source-derived**, not observed.
* No live capability run exists for `update.catalog.local`, `channel.radio`, `radio.status` or
  `channel.keyboard`; the audit's matrix predates all four. Whether `update check` lands on
  `available` or `limited` depends on the target's `ConfigUpdateRequired` value, which this repair
  did not read.
* `radio.status` availability depends on the WinRT radio session opening for the probing process; an
  error is recorded as `unavailable` with the error text (never skipped, never assumed).

## 6. Proposed parent acceptance (with negative controls)

1. `cargo check -p lctrl-hal -p vantage-cli --target x86_64-pc-windows-msvc` (the check this item
   asked for; not run here, see §5).
2. Build and re-run the matrix:
   `cargo.exe build --offline --manifest-path vantage\Cargo.toml --bin vantage`, then
   `vantage.exe --json capability > cap-after.json`.
   *Expect*: exactly the rows in §1/§2/§3 changed (`kbd backlight`, `events`, `storage status`,
   `sensor list`, `sensor read`, `bluetooth list`, `wifi status`, `radio status`, `update check`),
   plus rows added by peers; every other row byte-identical to
   `target-capability-matrix-20261105.json`.
3. Row-level expectations on the target: `kbd backlight` `available`; `radio status` `available`
   with a radio list detail; `radio off`/`radio on` `unverified`; `update check`
   `available`/`limited` with the "local synced catalogue … remote refresh requires an
   authenticated UDC contract" detail; `update local-install` and `update history` `unverified`;
   `doctor` still FAIL on `update.capability`.
4. Negative controls:
   * delete the `channel.keyboard` record locally (or point `role_channel(Keyboard)` at
     `channel.keyboard-missing`), rebuild: `kbd backlight` must fall back to `unverified` while
     `kbd.backlight` itself stays `available` — proves the role join (not the feature probe) is what
     was repaired, and that the mapping is load-bearing.
   * comment out the `update.catalog.local` record: `update check` must go back to `unverified`
     (not `available`, and not `unsupported`) — proves the row now depends on a real probe.
   * run `update check` on Linux: must print the capability-summary fallback, and the row must be
     `unavailable` (from the Linux recorder), never `available` and never `unsupported` — proves no
     fabricated catalogue and that the remote/local split holds on both backends.
   * `cargo test -p lctrl-hal` (registry contract) must stay green; no test outside
     `command_capability.rs` asserts an `Unprobed`/`Capability` distinction or the `UPDATE_CHECK`
     dispatch (grep-verified at the time of this note).
5. **Avoid** as a negative control: deleting/renaming the machine's `LastUpdateConfig`. It is a
   service-owned registry value and mutating it would destroy the only synced catalogue on the
   target. Use the Linux fallback row instead.

## 7. Remaining work (outside this change)

1. Live re-run of the matrix on the target (owner: parent) — converts every `after` above from
   source-derived to observed.
2. `update.history.ledger`: bounded precondition probe (`Test-Path` on
   `…\session\editable_update_history.db` / `update_history.db`, or `SELECT … LIMIT 1`) — owner:
   update family.
3. `radio.off`/`radio.on`: needs an executed `radio off --yes` acceptance with readback and
   rollback observed before an id can honestly be recorded.
4. `update.local_package`: needs a probe contract that does not require an operator-supplied
   directory (or an explicit decision not to have one).
5. Linux `channel.update-local` recorder, so the `update.*` rows stop reading `unverified` there.

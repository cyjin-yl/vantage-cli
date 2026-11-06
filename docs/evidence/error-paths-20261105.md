# Error paths: negative-control sweep of every implemented read command

Target: Lenovo 21VG (ThinkBook 14 G8+ IPH), Windows. Worktree
`/tank/shared/projects/vantage/replacement/target/bios-docs-20261030`, branch `main`.
Date: 2026-11-05/06.

Question: for every implemented **read** command, is the failure path honest?

1. an absent channel must yield `Unsupported` (exit 3) or `ChannelUnavailable` (exit 4),
2. a bad argument must yield `InvalidArgument` (exit 2),
3. nothing may exit 0 with empty (or meaningless) output when the channel is dead,
4. nothing may state a hardware/implementation absence that is false.

Method: (a) source-bound mapping of every read branch; (b) read-only execution of the
already-built target binary (`C:\Users\ezra\vantage-target\debug\vantage.exe`, length
10 413 056, mtime `2026-11-05T14:23:35+08:00`, SHA-256
`645B8B7BE73672290F9FE2D0FBC702DB39566E3A6D1666B5159F09270DA3E2DA`); (c) read-only
PowerShell/WMI recon where a channel verdict had to be established independently.
No write command was executed, no `--dry-run`-less mutating command was executed, no
file, service, device, registry value or hardware state was changed. The two
full-matrix reads were sampled twice: `capability` came back byte-identical, and
`doctor` reported the same `17/20 channels operational` with the same per-channel
verdicts both times — the only difference was the live list of competing vendor
processes inside `control.conflicts` (still `unavailable`), which is environment
state, not a verdict.

The binary predates properties added by sibling workers after 14:23 on 11-05
(`battery health`, `perf gpu`, `perf policy`, `privacy cam status`, `hotkey status`,
`update …`, `radio`). For those the entry is marked **source-bound**: the failure path
is established from the current source only, and is **not** claimed as observed.

Exit-code taxonomy under test is `LctrlError::exit_code`, `crates/lctrl-core/src/error.rs`
(Io=1, InvalidArgument=2, Unsupported=3, ChannelUnavailable=4, PermissionDenied=5,
FirmwareRejected=6, VerifyMismatch=7). Parse errors are intercepted in
`vantage/src/main.rs` (`parse_cli`): `--json` renders them as `InvalidArgument`,
text mode keeps clap's own exit 2.

## 1. Verdicts per read command

`src/lib.rs` is a shared file that sibling workers are editing concurrently, so it is
cited by function name rather than by line: line numbers observed during this sweep
moved twice in ten minutes.

| command | channel(s) read | observed on target (pre-fix binary) | absent-channel mapping (source) | verdict |
|---|---|---|---|---|
| `info` | `Hal::hardware_info/capabilities/identity` | exit 0, full identity + 103 feature rows | every read uses `?`; a failing HAL read propagates its own kind | honest |
| `doctor` | 20 optional channels | exit 0, `17/20 channels operational`, each unavailable row names its reason | `doctor_missing("…")` rows for unattached services (`execute_doctor`) | honest by design: it is a readiness matrix, not a command that claims a channel |
| `capability` | `Hal::capabilities` + command registry | exit 0, per-command matrix (`perf.fan.inventory available`, `perf.fan.mode unavailable (GAMEZONE …)`) | `hal.capabilities()?`; rows carry `Unsupported` reasons verbatim | honest by design |
| `battery` / `battery status` | 6 independent sources | exit 0, every source present, `*_error` all null | `None` service → `Unsupported("battery.status")`; all four primary sources failed → `ChannelUnavailable` (`battery_observation::execute`) | honest (**reference pattern**) |
| `battery health` | `BatteryStaticData`/`FullChargedCapacity`/`CycleCount` chain + vendor health | **source-bound** (not in binary) | `None` → `Unsupported("battery.health")`; capacity channel failed → `ChannelUnavailable` (`battery_health::execute`) | honest |
| `battery adapter` | vendor adapter + `GetSystemPowerStatus` | exit 0 | `None` → `Unsupported`; **both** sub-sources `None` → `ChannelUnavailable` (`execute_battery_adapter`) | honest |
| `battery temporary-mode` | `0x83102148` | exit 0, `raw=0x1 permanent=true …` | `None` → `Unsupported`; IOCTL failure propagates (`execute_temporary_charge`) | honest |
| `power scheme` (bare) | active scheme | exit 0, `active power scheme: 平衡` | `None` → `Unsupported("power.scheme")` (`execute_active_power_scheme`) | honest |
| `power scheme list` | enumerated schemes | exit 0, `1 power scheme(s)` | propagated | honest |
| `power scheme get <name>` | one enumerated scheme | exit 2, `invalid argument: power scheme "nosuch" is not enumerated` | `InvalidArgument` | honest |
| `perf fan` / `perf fan status` | fan mode + inventory | exit 0, **`1 fan device record(s); mode readback is unavailable`** while JSON carried `"mode":{"unknown":4},"mode_error":null` | `fans()` error was flattened by `unwrap_or_default()`; empty inventory rendered as a successful `0` | **DEFECT — fixed (§2.1)** |
| `perf temp` | ACPI zones, battery class, ESIF participants | exit 0, 1 measured zone + 1 sensor explicitly `availability: unavailable` | `None` → `Unsupported`; presenter refuses an empty set (`WindowsThermal::temperature_sensors`, empty ⇒ `Unsupported("perf.temp")`) | honest |
| `perf temp read <id>` | one sensor | exit 2, `invalid argument: temperature sensor "bogus" was not found` (`kind=invalid_argument` in JSON) | `InvalidArgument` (`WindowsThermal::temperature`) | honest |
| `perf dbdc [get]` | `LENOVO_REPORT_DBDC_DATA` | exit 0, 3 tiers, `system power 0 mW` on every tier | no instance → `ChannelUnavailable`; inconsistent arrays → `ChannelUnavailable` (`DbdcControl for WindowsThermal`) | honest: the zeros are the firmware's own `ROS_Power` array, published verbatim next to non-zero current limits — not a CLI default |
| `perf gpu [status]` | `IsSupportGpuOC` + OC parameter class | **source-bound** | `None` → `Unsupported("perf.gpu")`; `Data=0` reported as `supported:false` **with the probe bytes** (`execute_gpu_oc`) | honest (reports the probe, does not invent a capability) |
| `perf policy` | dispatcher `PowerSlider` values | **source-bound**; binary predates it | `None` → `Unsupported("perf.policy")`; an entirely absent `PowerSlider` key decodes to an empty policy and exits 0 (`read_policy`) | **report-only** (§4.3, owner = wave2-6) |
| `perf top` | Win32 process enumeration | exit 0, 10 rows, `2 skipped` reported | `None` → `Unsupported` | honest |
| `tune telemetry` | 10 sub-reads (4 tuning, 4 battery, performance, temperatures) | exit 0; `pl1_uw`/`pl2_uw`/`tau_us` printed `unavailable ({…"kind":"unsupported"…})`, other sources present | per-field nulls carry the structured error; the human view prints `unavailable (…)` for each (`execute_tune_telemetry`) | honest aggregate (exit 0 is correct: some sources answered) |
| `tune profile list` / `show <name>` | static ruleset | exit 0 / exit 2 `invalid argument: tuning profile "nosuch" was not found` | `InvalidArgument` for an unknown name | honest |
| `bios` / `bios list` | `Lenovo_BiosSetting` | exit 0, `34 BIOS setting(s)` | `None` → `Unsupported("bios")`; a query that fails propagates | honest on this machine; one latent path, see §4.1 |
| `bios get <name>` | one setting | exit 2, `invalid argument: BIOS setting "NoSuchSetting" was not found` | `InvalidArgument` (`execute_bios`, `BiosCommand::Get`) | honest |
| `bios password status` | `Lenovo_SetBiosPassword` metadata | exit 0 but text mode printed only `BIOS password status read` while JSON held `max_length/min_length/mode/supervisor_set` | `Unsupported` when the class publishes no instance (`WindowsBiosController::password_status`) | **DEFECT — fixed (§2.3)** |
| `panel brightness` (no percent) | WMI brightness | exit 0, `panel brightness: 100%` | `None` → `Unsupported("panel.brightness")`; read propagates (`execute_panel_brightness`) | honest |
| `touchpad status` | HID device leaves | exit 0, `touchpad: enabled` | `None` → `Unsupported` (`execute_touchpad`) | honest |
| `privacy status` | runtime devnodes + 3 BIOS switches | exit 0, `privacy status collected` (text mode carried **no state**; JSON carried all of it) | both services `None` → `Unsupported("privacy.status")` (`execute_privacy`) | **DEFECT — fixed (§2.2)** |
| `privacy cam status` | runtime + persistent camera | **source-bound** (binary predates it; `privacy cam status` was still parsed as a write and rejected exit 2) | both `None` → `Unsupported("privacy.cam.status")`; `--runtime/--persistent` on a status read → `InvalidArgument` (`execute_camera_status`) | honest |
| `sense status` | HPD RPC, persisted registry projection | exit 0, `source=hpd-rpc` | `None` → `Unsupported("sense.status")`; a failed live RPC falls back to the registry projection **labelled with its source**, and returns the channel error when neither is available (`WindowsSenseControl::sense_state`) | honest |
| `magicbay detect` | USB/ACPI enumeration | exit 0, `0 MagicBay device record(s) detected` | `None` → `Unsupported`; an empty USB enumeration is a truthful answer | honest |
| `magicbay lte status` | WWAN/MBIM | exit 3, `magicbay.lte: standard MBIM/UVC/DRM control is not attached` | `None` → `Unsupported` | honest |
| `wifi security` / `wifi history` | WLAN API | exit 0, `1 interface(s), 20 saved profile(s)` / `0 observation(s)` | `None` → `Unsupported("wifi.security")`; WLAN open/enumeration failure propagates | honest |
| `scan list` | diagnostic item registry | exit 0, 6 items | `None` → `Unsupported("scan")` | honest |
| `scan run <item>` | named diagnostics | exit 2, `invalid argument: unknown diagnostic item "bogus-item"` | `InvalidArgument` (`parse_diagnostic_kind`) | honest |
| `fix` / `fix scan` / `fix track` | declared ruleset, live observations, ledger | exit 0, `2 rule(s), 1 present` / `1 applied fix(es) on record` | `None` → `Unsupported("fix.*")` | honest |
| `scene list` / `scene track` | declared ruleset, ledger | exit 0, `4 declared scene rule(s)` / `0 applied scene action(s)` | `None` → `Unsupported("scene.*")` | honest |
| `scene test <event> [--dry-run]` | ruleset match | exit 0, `scene plan for …: rule … (1 of 4 rule(s) fire)`; unknown class → exit 0 `no scene fires on bogus.event.class` | no match is a legitimate answer | kept as-is by decision; doc note only (§4.2) |
| `hotkey list` | this process's registrations | exit 0, `0 live global hotkey(s)` | `None` → `Unsupported("hotkey")` | honest |
| `hotkey status` | Lenovo Fn package keys | **source-bound** (binary predates it) | `None` → `Unsupported("hotkey")`; a missing key/registry error is recorded in `notes`, never dropped (`NativeHotkey::status`, `crates/lctrl-hal-win/src/hotkey.rs`) | honest |
| `update history` | local Lenovo update ledger | exit 3, `update.history.without-catalog` | `Unsupported` | honest |
| `snapshot diff` | managed baseline file + live capture | exit 0, `snapshot comparison complete` | missing baseline → `Unsupported("snapshot.baseline")`; corrupt baseline → `InvalidArgument` (`read_managed_snapshot`) | honest |
| `daemon status` | named-pipe IPC probe + on-disk autostart record | exit 4 `channel is unavailable: vantage daemon IPC: … (os error 2)` on the **binary under test**; the current source answers exit 0 `running:false` + `observed_error` when nothing listens | `StatusProbe::Absent` is an answer, `StatusProbe::Fault` is `ChannelUnavailable` (`daemon_status`, `vantage/src/main.rs`) | honest (semantics changed after this sweep ran; see the note under this table) |
| streaming (`battery watch`, `tune watch`, `tune telemetry --interval`, `perf temp watch`) | one sample per interval | `--interval 0` rejected exit 2 by the clap range parser | `InvalidArgument` (`stream_spec`) | honest |
| no subcommand at all (`vantage`) | — | exit 2 + usage | clap `MissingSubcommand` | honest |

`daemon status` changed while this sweep was being written: the binary that was probed
returned exit 4 for an absent daemon, and the current implementation instead reports
`running: false` plus the error observed by that call (a daemon that is not running is a
state, not a channel fault), keeping `ChannelUnavailable` only for an endpoint that
exists but cannot complete the exchange (`StatusProbe::Fault`). The reference pattern for
"an action whose channel is absent" is therefore `daemon start` / `daemon stop`: they act
on the IPC channel and return `ChannelUnavailable` when nothing answers
(`daemon_request`, and `start_daemon` when the spawned daemon never becomes ready).

## 2. Silent-success / false-statement paths found, and the fixes

Three paths qualified. All three are text-mode honesty defects: JSON already carried
the truth in all three cases, but the operator-facing output did not — and one of the
three promoted a failed read into a successful count.

### 2.1 `perf fan` / `perf fan status` — dead inventory became "0 fans", and the human line lied about the mode

Pre-fix `execute_fan_status`:

```rust
let (mode, mode_error) = capture_optional(fan.fan_mode());
let fans = fan.fans().unwrap_or_default();          // <- error flattened
...
"{} fan device record(s); mode readback is unavailable\n"   // <- unconditional
```

Observed on the target (`sha256 645B8B7B…`):

```text
$ vantage perf fan            -> exit 0
1 fan device record(s); mode readback is unavailable
$ vantage --json perf fan     -> exit 0
{"fans":[{"id":1,"max_rpm":5100,"min_rpm":2100}],"mode":{"unknown":4},"mode_error":null}
```

Two defects in one line: the mode *was* read (`FanMode::Unknown(4)`, the dispatcher's
`ITS_FanMode=4` projection), yet the sentence asserted it was unavailable; and any
`FanControl::fans()` error — including the `Unsupported` a missing
`LENOVO_FAN_TEST_DATA` produces — was rendered as a successful zero-fan inventory.

Fixed (`execute_fan_status`): `fans()` is captured like `fan_mode()`; an empty inventory is
mapped to `Unsupported("perf.fan.inventory: firmware exposed no fan inventory record")`,
which is exactly what the capability probe already records for the same answer
(`record_fan_capabilities` in `crates/lctrl-hal-win/src/windows_hal.rs` — `fan_inventory_available = matches!(… Ok(objects) if
!objects.is_empty())`); **both** sub-reads unavailable now returns `ChannelUnavailable`;
the human line states the mode it actually read. JSON gains one field, `fans_error`,
next to the existing `mode_error`; no field was renamed or removed.

### 2.2 `privacy status` — text mode printed only "privacy status collected"

Observed:

```text
$ vantage privacy status        -> exit 0
privacy status collected
$ vantage --json privacy status -> exit 0
{"bios":[{"name":"IntegratedCamera","value":"Enable"},…],
 "runtime_camera":"enabled","runtime_microphone":"enabled"}
```

The command does read both layers correctly (both `None` ⇒ `Unsupported` is already
enforced), but text mode carried none of it: an operator saw a success line whether the
camera was enabled or every sub-read had failed.

Fixed (`execute_privacy`): the human view renders `runtime camera`, `runtime
microphone` (state, or `unavailable (<captured error>)`, or `service not attached`),
each persistent BIOS switch, and names any switch the firmware does not publish.
JSON is unchanged.

### 2.3 `bios password status` — text mode printed only "BIOS password status read"

Observed:

```text
$ vantage bios password status        -> exit 0
BIOS password status read
$ vantage --json bios password status -> exit 0
{"max_length":128,"min_length":1,"password_mode":1,"password_state":0,
 "supervisor_set":false,"supported_encodings":1,"supported_keyboard":1}
```

Fixed (`execute_bios`, `BiosCommand::Password { Status }`): the human view states `set: <bool>`, the length
bounds, and the observed status object. No secret material is printed — the payload is
the firmware's own switch metadata (length bounds, mode/state flags), not a stored
password.

## 3. Missing subcommand was classified as a hardware verdict (approved fix)

A bare parent command asked nothing of any channel, yet was reported as
`Unsupported`/exit 3 — and, for the parents without a dedicated arm, with a message
that asserted a hardware absence that is false. All observations below are from the
target binary (the `radio` row is source-bound: the binary predates it).

| invocation | before (observed) | after (source) |
|---|---|---|
| `vantage power` | 3 `power: a power subcommand is required` | 2 `a power subcommand is required; run `vantage power --help`` |
| `vantage perf` | 3 `perf: a performance subcommand is required` | 2 `a perf subcommand is required; …` |
| `vantage tune` | 3 `tune: a tuning subcommand is required` | 2 `a tune subcommand is required; …` |
| `vantage tune profile` | 3 `tune.profile: a profile subcommand is required` | 2 `a tune profile subcommand is required; …` |
| `vantage kbd` | 3 `kbd: a keyboard subcommand is required` | 2 `a kbd subcommand is required; …` |
| `vantage panel` | 3 `panel: a panel subcommand is required` | 2 `a panel subcommand is required; …` |
| `vantage privacy` | 3 `privacy: no verified implementation is attached on this platform` (**false**: `privacy status` works) | 2 `a privacy subcommand is required; …` |
| `vantage magicbay` | 3 `magicbay: standard MBIM/UVC/DRM control is not attached` (**false**: `magicbay detect` works) | 2 `a magicbay subcommand is required; …` |
| `vantage magicbay lte` | 3 `an lte subcommand is required` | 2 `a magicbay lte subcommand is required; …` |
| `vantage update` | 3 `update: no verified implementation is attached on this platform` (**false**: `update history`/`local-*` exist) | 2 `an update subcommand is required; …` |
| `vantage scan` | 3 `scan: no verified implementation is attached on this platform` (**false**: `scan list` works) | 2 `a scan subcommand is required; …` |
| `vantage snapshot` | 3 `snapshot: a snapshot subcommand is required` | 2 `a snapshot subcommand is required; …` |
| `vantage wifi` | 3 `wifi: a wifi subcommand is required` | 2 `a wifi subcommand is required; …` |
| `vantage radio` | 3 `radio: a radio subcommand is required` (**source-bound**) | 2 `a radio subcommand is required; …` |
| `vantage daemon` | 3 `daemon: daemon commands are handled by the platform binary` | 2 `a daemon subcommand is required; …` |

Implementation (`src/lib.rs`):
`missing_subcommand(&Command) -> Option<&'static str>` (added above `unsupported`)
names only the "nothing was asked" case, and `execute_with_services` returns
`InvalidArgument` before any dispatch or preflight. Parents that intentionally default
to a read are **not** listed (`battery` → status, `bios` → list, `hotkey` → list,
`fix` → rules, `scene` → list, `power scheme` → active scheme), and every genuine
`Unsupported` arm was left untouched: `battery thresholds`, `battery extreme-life`,
`power saver-once`, `tune turbo`, `kbd winlock`, `panel color/overdrive`,
`sense kbd-light-auto`, `magicbay detect/cam/display/watch/lte-control`,
`update download/install/…`, `radio off/on` on an unattached radio, `bios save`,
and the data-driven reasons already documented per domain.

The arms that existed only to produce the missing-subcommand verdicts were removed so
the two classifications cannot disagree: `Command::Power{None}`, `Perf{None}`,
`Tune{None}`, `Tune::Profile{None}`, `Kbd{None}`, `Panel{None}`,
`Magicbay::Lte{None}`, `Snapshot{None}`, `Wifi{None}`, `Radio{None}`, plus the
`Command::Magicbay { .. }` and `Command::Wifi { .. }` catch-alls whose text asserted a
non-existent absence (every remaining `Magicbay`/`Wifi`/`Radio` variant is either
dispatched or has its own reasoned arm).

Test consequence: `crates/vantage-cli/tests/cli_contract.rs`
`unsupported_error_uses_core_exit_code_mapping` used bare `tune` as its
`Unsupported` fixture. The assertion (CLI exit code equals the core mapping) is
unchanged; the fixture now parses `tune turbo on`, which is a genuine `Unsupported`
(`tune.turbo` with no processor-power service). No assertion was relaxed.

Documentation superseded by this change (owned by other workers, not edited here):
`docs/evidence/re-daemon-20261105.md:192` records bare `daemon` falling through to
`unsupported()` and proposes the one-line `Unsupported` fix; the actual fix is the
`InvalidArgument` rule above. `docs/evidence/doc-truth-20261105.md:81` likewise
describes `scan`/`daemon` as generic rejections. `docs/evidence/capability-audit-20261105.md`
§6.3 records bare `vantage power` as `EXIT=3` ("a power subcommand is required", VP-08)
and contrasts it with `power scheme`; the *contrast* still holds (`power scheme` is the
read form), but the exit code and kind are now 2 / `invalid_argument`. The selector
mapping that audit complains about (`command_id` sending bare `power` to
`POWER_SCHEME_ACTIVE`) is untouched.

## 4. Report-only findings (no change made)

### 4.1 `bios list` can silently drop rows

`WindowsBiosController::settings` in `crates/lctrl-hal-win/src/bios.rs` builds the list with
`filter_map(|object| match object.get("CurrentSetting") { Some(String) → parse_current_setting(..), _ => None })`:
an object whose `CurrentSetting` is not a string (or does not parse) disappears. A
provider that published only malformed rows would yield `0 BIOS setting(s)` at exit 0.
The query itself fails loudly when the class is absent, and on this machine the list
returns 34 settings, i.e. the filter has not been observed to drop anything here, so
the path is latent. Making it an error would change how a
single malformed firmware row is handled — an owner decision, not a sweep fix.

Also reachable-by-design: the empty-list branch of `execute_bios_list` prints
`0 BIOS setting(s)` at exit 0. Kept, because the capability
layer and the query error already distinguish "class absent" from "class publishes
nothing", and the branch is unreachable from the Windows HAL when the class is absent.

### 4.2 `scene test <undocumented event class>` exits 0 with `matched: 0`

`NativeScenePolicy::match_event` (`crates/lctrl-hal-win/src/scene_policy.rs`) compares strings against the
declared ruleset, so `bogus.event.class` answers
`{"event":"bogus.event.class","matched":0,"reason":"no declared scene rule fires on this event class"}`
at exit 0. Per the parent's decision this is a legitimate no-match for a runtime event
class and stays; recorded here so it is not later mistaken for a silent success. A
caller that wants argument validation instead must pass `--rule <id>`
(unknown id ⇒ `InvalidArgument`).

### 4.3 `perf policy` returns an empty policy instead of `Unsupported` when the channel is absent

`read_policy` (`crates/lctrl-hal-win/src/its_policy.rs`) decodes a machine whose `PowerSlider` key is
missing entirely into an empty `ItsPolicy` with no issues, so `perf policy` would exit 0
with every field "not published". This is documented as intentional in that module and
the row `perf.policy` is already reported `unavailable` by the capability probe; the
owner (wave2-6) has been asked by the parent to make the all-absent case an explicit
`Unsupported`. Not changed here.

### 4.4 Two remaining exit-0 aggregates (intentional)

`doctor` and `capability` exit 0 by contract — they exist to report which channels are
absent, and they name each absent one with its reason. `tune telemetry` exits 0 when at
least one source answered and labels each unavailable source with its structured error.
Neither is a dead-channel success.

### 4.5 Adjacent, not a read: `vantage osd` with no subcommand paints the test overlay

`execute_osd` defaults `command` to `OsdCommand::Test`, and
`command_is_mutating` does not list `Command::Osd`, so a bare `osd` performs the smoke
overlay without confirmation. Out of scope for a read sweep (it writes nothing to
hardware); flagged because it is the one bare parent that still defaults to an action
rather than a read.

## 5. Negative controls that passed unchanged

Bad arguments all yielded exit 2 (and `kind=invalid_argument` in JSON mode):

| invocation | observed |
|---|---|
| `perf pl1 abc` | 2 `invalid value 'abc' for '<WATTS>'`; `--json` → `{"kind":"invalid_argument",…}` |
| `battery charge-mode bogus` | 2 `invalid value 'bogus' for '<MODE>'` (possible values listed) |
| `kbd backlight zzz` | 2 `invalid value 'zzz' for '<LEVEL>'` |
| `perf temp watch --interval 0` | 2 `0 is not in 1..18446744073709551615` |
| `panel brightness 101` | 2 `101 is not in 0..=100` |
| `panel brightness --expect-current 50` | 2 required `<PERCENT>` missing (`requires`) |
| `bios get NoSuchSetting` | 2 `invalid argument: BIOS setting "NoSuchSetting" was not found` |
| `power scheme get nosuch` | 2 `invalid argument: power scheme "nosuch" is not enumerated` |
| `tune profile show nosuch` | 2 `invalid argument: tuning profile "nosuch" was not found` |
| `perf temp read bogus` | 2 `invalid argument: temperature sensor "bogus" was not found` |
| `scan run bogus-item` | 2 `invalid argument: unknown diagnostic item "bogus-item"` |
| `fix run bogus-issue --dry-run` | 2 `invalid argument: unknown fix issue bogus-issue; run `fix scan` …` |
| `vantage` (no args) | 2 + usage |

Absent-channel controls that passed unchanged: `magicbay lte status` → 3,
`update history` → 3, `bios save` → 3 (deliberate boundary), and every
`Option<&dyn …>`-guarded handler returns
`Unsupported` rather than an empty success (115 `ok_or_else(|| LctrlError::Unsupported`
sites at the time of this edit; the three exceptions found are §2). The channel-action
pattern for a *missing* daemon is `daemon start` / `daemon stop` →
`ChannelUnavailable` (`daemon_request`, `start_daemon`), not `daemon status`, which
reports the absent state at exit 0.

## 6. What remains unverified (parent owns these checks)

The fixes are source edits in a shared worktree; **nothing below was run**, by design of
this seat.

1. `cargo check -p vantage-cli --target x86_64-pc-windows-msvc` (cross) — new closure,
   `write!`/`writeln!` usage and the `missing_subcommand` match must compile.
2. `cargo test -p vantage-cli` — in particular the retargeted
   `unsupported_error_uses_core_exit_code_mapping`, plus `cli_contract` cases that pin
   `Unsupported` for genuinely unsupported verbs (they must be unaffected).
3. Build `vantage.exe` from this tree and re-run the probe list on 21VG:
   * `perf fan` must now print `fan mode: unknown (4)` **and** `1 fan device record(s)`;
   * `perf fan --json` must carry `fans_error: null` and an unchanged `fans` array;
   * `privacy status` must print the two runtime layers and the three persistent
     switches; `--json` must be byte-identical apart from ordering;
   * `bios password status` must print `set:`/length bounds plus the status object;
   * each parent in §3 must exit 2 and no longer claim a hardware absence;
   * negative controls that must NOT change: `usb` (bare) 2, `battery` (bare) 0,
     `bios` (bare) 0, `scene` (bare) 0, `hotkey` (bare) 0, `power scheme` 0.
4. A negative control the target cannot provide: a machine where
   `LENOVO_FAN_TEST_DATA` returns zero rows, to see `fans_error` populated and exit 3
   rather than "0 fan device record(s)". `FakeHal`/`Fake` fan doubles can drive this in
   `crates/vantage-cli/tests/cli_contract.rs` (no such test exists today).

## 7. Provenance

Probe scripts and full transcripts (four PowerShell passes plus the raw `--json`
bodies) were kept outside the repository (`/tmp/ep-sweep/`), because the
`battery status` payload contains the pack serial number. Nothing from that payload is
reproduced here beyond field names and null/available structure.

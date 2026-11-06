# GUI parity and crash surface for the 2026-11-06 command set — 2026-11-06

Scope: whether the commands added in this work window are **reachable/represented** in
`crates/vantage-gui` (and in any TUI), and whether they can **crash** it (unwrap / expect /
panic / index). The only permitted source-edit class was "a panic or unwrap that a new command
can newly trigger". **No source edit was required, and none was made** (§5 says why, per finding).

Every claim below is either a read of the named bytes or an explicit `[INFERENCE]`. This seat is
forbidden by its instructions from running `cargo check`/`cargo test`/clippy; §4.4 therefore hands
the two compile commands to the parent verbatim, and §4.1–§4.3 give the static evidence that
substitutes for a type check of GUI-owned code.

## 0. Evidence binding

Read on 2026-11-06 11:42–12:2x CST. `windows_hal.rs` and `lctrl-hal-linux/src/lib.rs` are being
edited by other seats while this audit ran (§0 hashes are the exact bytes read; line numbers drift
with the next sibling edit).

| file | mtime | sha256 (first 16) |
|---|---|---|
| `crates/vantage-gui/src/lib.rs` | 2026-11-01 23:01:19 | `c1323a07771977a0` |
| `crates/vantage-gui/src/main.rs` | 2026-11-01 23:00:59 | `6846931b7431347f` |
| `crates/vantage-gui/Cargo.toml` | 2026-11-04 01:33:10 | `0b2ac67dc3edfb95` |
| `crates/lctrl-core/src/capability.rs` | 2026-11-05 23:00:57 | `23c85c5eef2baeee` |
| `crates/lctrl-hal/src/capability.rs` | 2026-11-06 03:04:53 | `465ef485b6d6b8e4` |
| `crates/lctrl-core/src/command_capability.rs` | 2026-11-06 10:31:20 | `057bd1b3cb3cecd5` |
| `crates/lctrl-hal/src/command_capability.rs` | 2026-11-06 11:38 | `8c37c07bedc12797` |
| `crates/lctrl-hal-win/src/windows_hal.rs` | 2026-11-06 11:37:13 | `711133c09b9d58b6` |
| `crates/lctrl-hal-linux/src/lib.rs` | 2026-11-06 12:14 | `930e2173ffcde11c` |

`git status` reports **no modification and no new file** under `crates/vantage-gui/`: the GUI has
not been touched since 2026-11-01, five days before this window. Nothing in this audit describes a
GUI edit made today; it describes how today's *command/HAL* edits land on an unchanged GUI.

## 1. How the GUI enumerates commands — it does not (step 1)

There is **no command list of any kind** in the GUI. `grep -rn CommandId crates/vantage-gui/`
returns nothing (exit 1). `vantage-gui` does not depend on `vantage-cli`, does not derive clap, and
`crates/vantage-gui/Cargo.toml` pulls only `gpui =0.2.2`, `lctrl-core`, `lctrl-hal`, and (per
target) `lctrl-hal-linux` / `lctrl-hal-win`.

The GUI's entire data model is a `CapabilitySet`:

* `DashboardSnapshot::from_hal` (`lib.rs:54`) calls exactly three HAL methods —
  `hal.platform()`, `hal.hardware_info()?`, `hal.capabilities()?`.
* `capability_matrix` (`lib.rs:357`) renders **every entry of `CapabilitySet::features`**, i.e. a
  `BTreeMap<String, Capability>` keyed by *feature id* (`crates/lctrl-core/src/capability.rs:28-31`).
  The header count is `features.len()` (`lib.rs:424`); an empty set renders an explicit "No
  capability claims available" row instead of nothing.
* `telemetry_panel` (`lib.rs:442`) counts the three `Availability` values over the same map.
* The sidebar (`lib.rs:164`) is a hardcoded, purely decorative 7-item list (`OVERVIEW … DIAGNOSTICS`)
  with `OVERVIEW` the only active item; it is not bound to commands, features, or routing.

So the enumeration question is answered structurally: **the GUI is capability-driven, not
command-driven**; its "surface" is whatever rows the attached HAL records. `main.rs` has no
dispatch path at all — it builds one HAL, takes one snapshot, and opens one window
(`main.rs:5-14`, `main.rs:25-33`).

**TUI:** none exists. `grep -rn "tui\|ratatui\|crossterm" --include=Cargo.toml crates/ vantage/`
matches nothing; the `vantage` binary is the clap CLI (`vantage/src/main.rs`), and
`crates/vantage-cli` is the clap definition + handlers. `crates/vantage-daemon` defines no UI and
is not linked by `vantage-gui`. There is therefore no second hand-maintained command list anywhere
in the tree to fall out of sync.

## 2. Every CommandId added today, and what the GUI shows for it (step 2)

`git log --since="2026-11-05" -p -- crates/lctrl-core/src/command_capability.rs` adds **zero**
`CommandId` constants. All 25 additions are uncommitted working-tree edits
(`git diff … | grep -c '^+.*pub const [A-Z]'` = 25; the same diff removes none).

Because the GUI renders feature ids, the honest parity object is each new command's
`FeatureBinding` (`crates/lctrl-hal/src/command_capability.rs`) against the rows the two HALs
actually record. `W` = a capability row is recorded by `lctrl-hal-win` (so the Windows GUI shows a
row); `L` = the same for `lctrl-hal-linux`.

| # | `CommandId` (wire) | `FeatureBinding` (hal registry line) | `W` row site | `L` row site | what the GUI shows |
|---|---|---|---|---|---|
| 1 | `cpu.status` | `Capability("cpu.status")` 272 | `windows_hal.rs:1126` | — | row on Windows: the command's own clock availability |
| 2 | `events.log` | `Capability("events.log")` 284 | `event_log.rs:522,542,677` | — | row on Windows |
| 3 | `battery.health` | `Capability("battery.health")` 320 | `windows_hal.rs:275` | `lib.rs:649` | row on both |
| 4 | `usb.ports` | `Capability("usb.ports")` 453 | `usb_ports.rs:99,107,112` | — | row on Windows |
| 5 | `power.ac` | `Capability("battery.adapter")` 548 | `windows_hal.rs:367` | `lib.rs:662` | row on both (shared with `battery adapter`) |
| 6 | `power.draw` | `Capability("battery.status")` 560 | `windows_hal.rs:350` | `lib.rs:635` | row on both |
| 7 | `perf.gpu.status` | `Capability("perf.gpu")` 750 | `windows_hal.rs:824` | — | row on Windows |
| 8 | `perf.policy` | `Capability("perf.policy")` 758 | `its_policy.rs:94,101` | — | row on Windows |
| 9 | `touchpad.caps` | `Capability("touchpad.caps")` 1047 | `windows_hal.rs:630` | — | row on Windows (Linux records `touchpad`, not `touchpad.caps`) |
| 10 | `panel.info` | `Capability("panel.identity")` 1071 | `windows_hal.rs:766` | — | row on Windows |
| 11 | `audio.endpoints` | `Capability("audio.endpoints")` 1419 | `windows_hal.rs:1374` | — | row on Windows |
| 12 | `audio.volume` | `Capability("audio.volume")` 1427 | `windows_hal.rs:1374` | — | row on Windows |
| 13 | `audio.mute` | `Capability("audio.mute")` 1439 | `windows_hal.rs:1374` | — | row on Windows |
| 14 | `firmware.status` | `Capability("firmware.status")` 1563 | `windows_hal.rs:198` | — | row on Windows |
| 15 | `hotkey.status` | `AnyOf(["channel.hotkey","kbd.fnlock"])` 1738 | `windows_hal.rs:1448,1808/1819/1831` | `lib.rs:913` (`kbd.fnlock`, Unavailable) | row(s) on both |
| 16 | `update.local.package` | `Unprobed("update.local_package")` 1832 | — | — | **no row; mutation** |
| 17 | `wifi.status` | `Unprobed("wifi.status")` 1985 | `windows_hal.rs:1182` | — | row on Windows |
| 18 | `bluetooth.list` | `Unprobed("bluetooth.list")` 1993 | `windows_hal.rs:1200` | — | row on Windows |
| 19 | `sensor.list` | `Capability("sensor.list")` 2005 | `windows_hal.rs:1236` | — | row on Windows |
| 20 | `sensor.read` | `Capability("sensor.list")` 2013 | `windows_hal.rs:1236` | — | row on Windows |
| 21 | `security.status` | `Capability("security.status")` 2021 | `windows_hal.rs:1260` | — | row on Windows |
| 22 | `radio.status` | `Unprobed("radio.status")` 2033 | `windows_hal.rs:1298` | — | row on Windows |
| 23 | `radio.off` | `Unprobed("radio.off")` 2049 | — | — | **no row; mutation** |
| 24 | `radio.on` | `Unprobed("radio.on")` 2063 | — | — | **no row; mutation** |
| 25 | `storage.status` | `Capability("storage.status")` 2190 | `windows_hal.rs:481` | — | row on Windows |

**Verdict on omission.** The GUI omits *all 25 as commands*, and that is **safe by design, not a
defect**, because the GUI has no command surface to omit them from: it is documented and coded as a
read-only observer (`lib.rs:1-6`; `lib.rs:241-256` prints "● OBSERVE ONLY" and "Controls unlock
only after a safe dispatcher is wired"). Three consequences of that design are worth stating rather
than "fixing":

1. 21 of the 25 read verbs appear as capability rows automatically, on the platform that records
   them, with **no GUI source change** — that is the parity mechanism working as intended.
2. 4 of them (`update.local.package`, `radio.off`, `radio.on`, plus the mutation halves of
   `audio.volume`/`audio.mute` that share a row with their read verb) produce **no row**, and 3 of
   those are state-mutating verbs the read-only GUI must not present. No row is the correct
   rendering of "this shell will never do that".
3. The remaining absence is a *platform* fact, not a GUI fact: on Linux only
   `battery.health`, `battery.adapter`, `battery.status`, `kbd.fnlock` of the new feature ids have
   rows, so a Linux GUI shows no `cpu.status`/`storage.status`/`security.status`/`wifi.status`/…
   row. That mirrors the Linux HAL owning no backend for those channels; it is not a GUI gap and
   the pane cannot fabricate a row for it.

Two presentation limits to record, not fix: (a) the sidebar sections never change, so on a real
machine all rows land in the single `CAPABILITY MATRIX` block regardless of section; (b) the row
set is exactly what `capabilities()` recorded, so a row is a *probe* result, not a command list.

## 3. Crash surface (step 3)

### 3.1 The GUI's own code has no panic path

`grep` for `unwrap()|\.expect(|panic!|unreachable!|assert|todo!|\[0\]` over `lib.rs:1-525` (all of
the file before `#[cfg(test)]` at `lib.rs:526`) and over the whole of `main.rs` returns **no
matches**. The only "unwrap"-family calls are five `unwrap_or(...)` fallbacks for optional strings
(`lib.rs:268,273,278,363`), which cannot panic.

The three structural safety properties, all in GUI-owned code:

1. **Error propagation, not unwrapping.** `from_hal` uses `?` on both fallible HAL calls
   (`lib.rs:55-57`). `main.rs:25` converts any `Err` into an explicit
   `DashboardSnapshot::unavailable(platform, "Hardware probe failed: {error}")`, and `run()`
   returns `LctrlError` for the no-display case (`lib.rs:85-92`) rather than unwrapping. A newly
   `Unsupported` channel therefore degrades to a banner; it cannot abort.
2. **Exhaustive matches on the closed enums.** `platform_name` (`lib.rs:511`) matches
   `Platform::{Windows,Linux}`; `availability_presentation` (`lib.rs:518`) matches
   `Availability::{Available,Limited,Unavailable}`. `crates/lctrl-core/src/capability.rs:7-18`
   still has exactly those variants today, so nothing changed; and if a variant were added, the GUI
   would fail to **compile**, not panic at render time. (No `_ =>` arm exists, verified at both
   sites.)
3. **No hardware mutation path.** `vantage-gui` links no control trait other than `Hal`, and calls
   nothing on it beyond the three read methods above.

The test-only `.expect(…)` calls at `lib.rs:562-568,596` and the deliberate
`panic!("capabilities must not be queried after hardware failure")` double at `lib.rs:591` live
behind `#[cfg(test)]` and are assertions *about* the GUI, not reachable from a running GUI.

### 3.2 The only third-party code a new command can newly push through the GUI

The GUI executes new command code in exactly one place: `capabilities()`. On Windows that function
runs the probe for essentially every channel, including every channel added in this window. The
GUI's other HAL calls are `hardware_info()` (three CIM/`Win32_BIOS` reads) and `platform()`.

`crates/lctrl-hal-win/src/windows_hal.rs` contains **zero** `unwrap()` / `.expect(` / `panic!` /
`unreachable!` / `todo!` — every fallible probe is matched and recorded with `Availability::
Unavailable` plus the error text, and `CapabilitySet::record(...)?` is the only `?`-propagated call
(the only way it errors is an empty feature id, and every literal id in the file is non-empty).
The probe helpers it calls were added/edited today and are captured in the table below.

### 3.3 Every production panic-family site in the crates the GUI links

Method: for each `crates/lctrl-hal-win/src/**`, `crates/lctrl-hal-linux/src/**`,
`crates/vantage-gui/src/*.rs`, everything after the first `#[cfg(test)]` (including the
`#[path = "…_tests.rs"]` modules `firmware_tests`, `security_tests`, `mobile_broadband_tests`,
`native_battery_electrical_tests`) is excluded. `lctrl-hal-linux` has exactly **one** site; the GUI
has none; `lctrl-hal-win` has the following 36, with the guard that decides reachability:

| site | construct | guard / reachability |
|---|---|---|
| `display.rs:108` | `.expect("length checked")` on `next()` | inside `match objects.len() { 1 => … }`; the arm is only taken at len 1 |
| `display.rs:744,748` | `try_from(size_of::<T>()).expect` | `size_of` of a fixed struct; infallible |
| `display_status.rs:460,501,538,586` | `try_from(size_of::<…>()).expect` | fixed struct sizes; infallible |
| `fn_osd.rs:290` | `try_from(size_of_val(&u32)).expect` | 4 bytes; infallible |
| `hotkey.rs:640,667` | `try_from(size_of_val(…)).expect` | fixed buffer sizes; infallible |
| `hotkey.rs:171,192,201,267,279` | `REGISTERED/LAST_HOTKEY.lock().unwrap()` | **Mutex poisoning only**; reached by `hotkey register/unregister/list` and the WndProc, not by `capabilities()` |
| `magicbay.rs:50,141,174` | `try_from(…).expect` | fixed struct / `Vec` len < u32::MAX; infallible |
| `msr.rs:137,138,183` | `try_from().expect("len checked")` on fixed slices | lengths fixed by the IOCTL buffer; infallible |
| `native_battery_electrical.rs:95` | `.unwrap()` on `status[o..o+4]` | guarded by `if status.len() != 16 { return Err(…) }`; offsets 0–16 all in bounds. **On the GUI path** (`capabilities()` calls `electrical.read_electrical()`) but unreachable |
| `native_battery_electrical.rs:354` | `.unwrap()` on `info[..4]` | `query::<36>` returns `[u8;36]`; infallible |
| `native_cpu.rs:76` | `try_from(size_of_val(&u32)).expect` | 4 bytes; infallible |
| `native_performance.rs:26` | `try_from(size_of_val(&u32)).expect` | 4 bytes; infallible |
| `native_power.rs:36` | `try_from(size_of::<GUID>()).expect` | fixed struct; infallible |
| `ndr/elevoc.rs:184,200,286` | `try_from(len).expect` on statically sized NDR format tables | table lengths are compile-time constants |
| `osd.rs:96,120,187,203,246` | `OSD_TEXT/CURRENT.lock().unwrap()` | **Mutex poisoning only**; reached by `osd show/test`, not by `capabilities()` |
| `power_tuning.rs:48,49` | `PowerGuid::new(<const literal>).expect` | literal GUIDs; infallible |
| `process.rs:912` | `unreachable!("bounded process image query returns or resizes once")` | reached only by `perf top` process enumeration; **not** on the `capabilities()` path |
| `runtime_devices.rs:960,1148,1306,1319` | `try_from(…)` size/len `expect` | fixed struct sizes; infallible |
| `scene_policy.rs:68,82,96,106` | `.expect("well-formed rule")` | four literal rule constructors; reached by `scene test/track`, not by `capabilities()` |
| `sense_rpc.rs:78` | `.expect("HPD session initialized")` | **On the GUI path** (`capabilities()` calls `read_state()`); guarded by the immediately preceding `if slot.is_none() { *slot = Some(Session::connect()?) }` — the `?` returns early on connect failure, so the slot is `Some` at the `expect` |
| `touchpad_caps.rs:661` | `try_from(size_of::<HIDD_ATTRIBUTES>()).expect` | fixed struct; infallible |
| `lctrl-hal-linux/src/lib.rs:244` | `unreachable!("binary mode values were checked")` | inside `if conservation <= 1 && rapid <= 1`, so every reachable tuple is one of the four non-wildcard arms. Reached by `battery charge-mode`, **not** by `capabilities()` |

### 3.4 Why none of them is newly triggerable by a new command

* The two sites actually **on** the GUI's startup path — `sense_rpc.rs:78` and
  `native_battery_electrical.rs:95` — are both protected by a guard on the same value they index
  (an `Option` just filled, a slice length just checked). They are "checked-by-construction"
  expects, not hazard sites, and they predate this window (`sense_rpc.rs` last touched 2026-11-06
  04:00, `native_battery_electrical.rs` 2026-11-01).
* Every remaining site is either a `size_of`/`try_from` constant-fold (cannot fail on either
  target), a literal-argument `.expect`, a mutex-poison unwrap on a path the GUI never takes, or a
  `unreachable!` behind a runtime invariant in a command the GUI cannot call.
* The GUI never sees a `Result` from a command: it sees a `CapabilitySet` whose construction is
  total. `Availability` is a closed 3-variant enum, so "a newly `Unsupported` command" arrives at
  the GUI as `Availability::Unavailable` + `detail` text, which `capability_matrix` renders as a
  string.

### 3.5 Consequences that are *not* panics (reported, not fixed)

1. **A poisoning panic is latent, not new.** If any thread panics while holding `OSD_TEXT`,
   `CURRENT`, `REGISTERED` or `LAST_HOTKEY`, the next `osd`/`hotkey` call unwraps the poisoned
   mutex and aborts that CLI process. The GUI never calls those paths, so it is not a GUI crash.
   Out of this item's fix class.
2. **`capabilities()` grew into a heavy startup probe.** On Windows the GUI now runs, before the
   window opens: ~40 CIM/WMI queries, `netsh` (`NetshCommandRunner::show_interfaces`, which spawns
   a process), Bluetooth enumeration, Win32 sensor enumeration (one read per sensor), SetupAPI PnP
   walks, `MMDevice` enumeration, and two COM/RPC `load()`s (Dolby, Elevoc). No new panic, but the
   first frame of `vantage-gui` is blocked on all of it. This is a *liveness* property a parent
   check can measure (§7); it is not a parity defect.
3. **The no-display guard is Linux-only.** `run()` short-circuits on missing `DISPLAY`/
   `WAYLAND_DISPLAY` under `cfg!(target_os = "linux")` (`lib.rs:85`), so on Windows the
   "graceful `ChannelUnavailable` instead of crashing" promise rests entirely on
   `cx.open_window` returning `Err` inside `Application::run`. That is gpui-supplied and
   **unverified on the target**; it is unchanged by any today edit.

## 4. Compile status (step 4)

### 4.1 Symbol audit — what the type check of GUI-owned code would cover

`vantage-gui` compiles against `lctrl-core`, `lctrl-hal`, and one HAL. Every symbol it names was
re-verified against today's bytes, all present with compatible shapes:

| GUI use | definition today | status |
|---|---|---|
| `lctrl_core::{Availability, CapabilitySet, HardwareInfo, Platform, Result, LctrlError}` | `crates/lctrl-core/src/lib.rs` re-exports all six from `capability.rs`/`error.rs` | ✅ |
| `CapabilitySet::{new, record, get, features}` | `capability.rs:28-69` (`features: BTreeMap<String, Capability>` is `pub`) | ✅ |
| `HardwareInfo` 3 × `Option<String>` + `Default` | `capability.rs:71-76` | ✅ |
| `Availability` 3 variants | `capability.rs:14-18` | ✅ (closed match still exhaustive) |
| `Platform::{Windows,Linux}` | `capability.rs:7-10` | ✅ (closed match still exhaustive) |
| `LctrlError::{Unsupported{feature}, ChannelUnavailable{channel}}`, `exit_code() -> u8` | `error.rs:5-33` | ✅ (`main.rs:41` does `i32::from`) |
| `lctrl_hal::Hal` | `crates/lctrl-hal/src/capability.rs` | ✅ |
| `lctrl_hal_win::{NativeWindowsHal, NativeWmi, NativeIoctl}` | `lib.rs:204,290,314`; `NativeWindowsHal = WindowsHal<NativeWmi, NativeIoctl>` and `WindowsHal<W,I,E = NativeBatteryElectrical>` keeps the 2-arg `new` (`windows_hal.rs:24-36`) | ✅ |
| `lctrl_hal_linux::LinuxHal::new()` | `lib.rs:93-98`, `impl Hal for LinuxHal` at `612` | ✅ |

### 4.2 The one trait-growth risk, and it is defused

`Hal` grew two methods in this window (`system_identity`, `cpu_status`,
`crates/lctrl-hal/src/capability.rs:5-24`) — both **defaulted**. That matters because the GUI's
`#[cfg(test)]` double `ReadOnlyHal` implements only the original three methods
(`lib.rs:534-552`). Because the new methods have bodies, the double still satisfies `Hal`, so
`cargo test -p vantage-gui` is not broken by the trait growth. Had they been required methods, the
GUI lib would still have built but its test target would not.

### 4.3 On-disk build evidence (not a run performed here)

* **Host target, vantage-gui** — `target/debug/.fingerprint/vantage-gui-{87ab5b44…,26e271e0…,
  6de7ec72…,a254016f…}` all carry their success markers and `libvantage_gui-87ab5b44….rmeta`
  (52 763 B) exists, written **2026-11-06 10:37–10:38**: lib, bin, test-lib and test-bin all
  type-checked on the host an hour after this window's first edits. Its `deps` list is
  `gpui, lctrl_core, idna_adapter, lctrl_hal, lctrl_hal_linux` — the Linux closure, as expected.
  **Staleness boundary:** that check predates `privacy.rs` (11:16), `lctrl-hal/src/
  command_capability.rs` (11:38) and `lctrl-hal-linux/src/lib.rs` (12:14), so it cannot certify
  today's tree.
* **`x86_64-pc-windows-msvc` target, vantage-gui** — **unverified, and the tree shows no completed
  unit.** `target/x86_64-pc-windows-msvc/debug/deps` contains no `gpui`/`vantage_gui` metadata
  (only `gpui_collections`), the fingerprint dirs `gpui-{3ee72042…,88910c8c…}` and
  `vantage-gui-{25b094ef…,bd898f78…}` are **empty directories created 2026-11-05 18:41**, and
  gpui's dependency units for msvc (`taffy`, `usvg`, `resvg`, `lyon`, `image`) are likewise empty
  dirs from the same minute. Cargo pre-creates those directories when it plans the unit graph, so
  this records "planned on 11-05 18:41, never executed to completion" — not a failure signature
  either. `lctrl-core`/`lctrl-hal` *did* complete for msvc (metadata at 11:38) and `lctrl-hal-win`
  is mid-build as of 12:13, so the msvc tree is live and the GUI units are simply the part that has
  never run.
* Toolchain feasibility for the parent's run: the `stable` toolchain has
  `manifest-rust-std-x86_64-pc-windows-msvc` installed, and gpui's `windows` resource embedding is
  guarded by `#[cfg(target_os = "windows")]` **in its build script**, i.e. by *host* OS — so
  cross-checking from this Linux host does not need `llvm-rc`/`windres` (neither is on PATH).

### 4.4 Commands for the parent (not run by this seat)

```bash
cargo check -p vantage-gui --target x86_64-pc-windows-msvc
cargo check -p vantage-gui --all-targets                 # host: also type-checks ReadOnlyHal
```

Note for interpreting the result: on Windows the GUI's only direct dependencies are `gpui`,
`idna_adapter`, `lctrl-core`, `lctrl-hal`, `lctrl-hal-win`, and the whole of `lctrl-hal-win` must
compile first. A failure in `usb_ports.rs`, `radio.rs` or `event_log.rs` (sibling work in flight as
of 12:1x) fails the *GUI* command without implicating the GUI. Only an error located in
`crates/vantage-gui/**`, `vantage-gui`'s dependency list, or a mismatched `lctrl-*` signature is a
GUI-parity finding.

## 5. What was changed — nothing

No file in `crates/vantage-gui/` and no HAL entry was edited by this seat. Justification, stated as
the negative result of the two searches the item asked for:

1. No GUI code path calls a command, capability *result* unwrap, or `Result` accessor at all
   (§1, §3.1) — there is nothing for a new command to newly unwrap.
2. The only new-command code a GUI can reach is `capabilities()`, which is panic-free, and the two
   `expect`s inside it are guarded by the same expression they unwrap (§3.2–§3.4).
3. Therefore the permitted fix class — "panics or unwraps that a new command can newly trigger" —
   is empty. Editing `sense_rpc.rs:78` or `native_battery_electrical.rs:95` to satisfy a grep would
   be a change with no observable behaviour, on files owned by other seats, in a shared tree.

`docs/evidence/gui-parity-20261106.md` is the only artifact created.

## 6. Observations for other owners (report-only, no edit made)

1. **`Unprobed` labels for three features that now have probes.** `wifi.status` (hal registry
   `1985`), `bluetooth.list` (`1993`) and `radio.status` (`2033`) are bound as `Unprobed("x")`
   while `windows_hal.rs` records `x` at `1182`, `1200`, `1298` respectively. The variant's own doc
   says "no current capability probe" (`crates/lctrl-hal/src/command_capability.rs:102-103`), which
   is now untrue. **Impact: none.** `feature_presence` resolves `Unprobed(f)` exactly like
   `Capability(f)` — both call `capability_presence(f, …)`
   (`crates/vantage-cli/src/capability.rs:1117-1124`) — so the CLI verdict and the GUI row are
   already correct; only the registry label is stale. Owner: the `lctrl-hal` command-registry seat.
2. **The capability matrix cannot distinguish 78 rows by section.** On Windows today
   `windows_hal.rs` + `its_policy.rs` + `event_log.rs` + `usb_ports.rs` + `fn_osd.rs` have 92
   `record` call-sites over 78 distinct ids (the 14 duplicates are mutually exclusive
   if/else/`Err` arms — verified by extraction, e.g. `panel.refresh` 3×, `hotkey.status` 3×), so
   the GUI header reads "78 SIGNALS". If the GUI is ever meant to page by sidebar section, that is
   a feature decision, not a bug in today's work.
3. **Ideal-but-out-of-class:** the Windows GUI pays the full probe cost at startup (§3.5.2); the
   Linux-only `DISPLAY`/`WAYLAND_DISPLAY` guard (§3.5.3) has no Windows counterpart.

## 7. Proposed parent verification and negative controls

| # | check | what a pass proves |
|---|---|---|
| 1 | `cargo check -p vantage-gui --target x86_64-pc-windows-msvc` | §4.3's never-run unit finally completes: GUI + gpui compiles for the target it ships on |
| 2 | `cargo check -p vantage-gui --all-targets` | the `ReadOnlyHal` test double still satisfies the grown `Hal` (§4.2) |
| 3 | `cargo test -p vantage-gui` on the host with `DISPLAY=` unset | `run()` returns `ChannelUnavailable` instead of panicking; the three existing tests (`snapshot_preserves_unavailable_capability_detail`, `from_hal_keeps_core_errors_strict`, `desktop_session_requires_nonempty_display_or_wayland_socket`) still hold |
| 4 | negative control on target: run `vantage-gui.exe` from an SSH session (no interactive desktop) | shows whether the unguarded Windows path degrades or aborts (§3.5.3) — this is the one *unverified* crash claim in this document |
| 5 | bounded timing: `vantage info --json` vs. GUI startup on `tb14p-ezra` | quantifies §3.5.2's probe cost so it can be judged rather than guessed |
| 6 | `cargo test -p lctrl-hal-win` (target-independent parts) after the in-flight `usb_ports/radio/event_log` edits settle | the capability rows in §2's table are actually produced, i.e. the GUI's 78 signals are not a promise |

## 8. Remaining uncertainty (stated, not resolved)

* `[INFERENCE]` The Windows GUI *renders* correctly for the new rows; I verified the row is
  recorded and that the renderer is total, but no screenshot or target run was performed (and this
  seat must not start one).
* `[UNVERIFIED]` gpui 0.2.2's Windows window-creation error path (§3.5.3), and whether
  `cargo check --target x86_64-pc-windows-msvc` for `gpui` succeeds at all in this tree (§4.3).
  Both are parent-run checks 1 and 4.
* Line numbers in §2/§3 are bound to the hashes in §0; `windows_hal.rs` was still being written
  11:37 and `lctrl-hal-linux/src/lib.rs` at 12:14, so re-anchor before quoting them downstream.

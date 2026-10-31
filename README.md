# vantage

Control your Lenovo ThinkBook from the terminal: performance mode, battery,
screen, keyboard, BIOS settings, updates, and diagnostics — without the vendor
app, without an account, without telemetry.

Every write is checked by reading the value back. If a channel isn't verified
on your machine, the command says so and refuses instead of pretending.

## Install

**Windows** — download `vantage.exe` from [Releases](https://github.com/cyjin-yl/vantage-cli/releases)
(also kept at `dist/windows/vantage.exe` on every tag), verify the SHA-256 in
`dist/windows/vantage.exe.sha256`, and run it from anywhere. No installer, no
background service. Reading state needs no elevation; changing hardware may
require an admin shell and always requires `--yes`.

**Linux** — build from source (see [Development](#development)). Some controls
that are vendor-only on Windows use standard kernel interfaces on Linux.

## Quick start

All read-only, safe to explore:

```bash
vantage info            # what machine is this, what's available
vantage doctor          # probe the channels and report problems
vantage capability      # per-command status matrix for this machine
vantage battery status  # charge, health, adapter
vantage perf temp       # CPU temperature
```

Your first change — preview it, then apply it:

```bash
vantage --dry-run perf mode performance   # show the plan, write nothing
vantage --yes perf mode performance       # apply, read back, confirm
```

Global flags: `--json` (machine-readable output), `--dry-run` (validate
without writing), `--yes` (confirm a write after reviewing it). Every
subcommand accepts `-h`.

## Commands by task

| Task | Commands |
|---|---|
| Performance & power | `perf mode`, `tune epp`, `tune turbo`, `power scheme list/get/set`, `perf temp` |
| Battery | `battery status`, `battery adapter` |
| Screen | `panel rate`, `panel brightness` |
| Input | `touchpad status/on/off/recover`, `kbd backlight` |
| BIOS | `bios list`, `bios get <name>` |
| Updates | `update check`, `update local-driver` (install/rollback vendored drivers) |
| Network | `wifi security` (audit live and saved networks) |
| Diagnostics | `doctor`, `fix scan/run`, `scan`, `snapshot capture/diff/restore` |
| Desktop integration | `osd`, `hotkey`, `scene`, `daemon` |

## What works on this machine

`vantage capability` is the source of truth — run it. Verified on the
ThinkBook 14 G8+ (21VG): performance mode (`auto`/`quiet`/`performance`),
processor energy preference and boost policy, Windows power schemes, built-in
panel brightness and refresh rate, touchpad recovery, multi-source battery
readings, Wi-Fi security audit, MachineFix repair rules, OSD / hotkey / scene
automation, BIOS setting reads, and hardware snapshots.

## Known limits — and why

| Feature | Status | Why |
|---|---|---|
| Charge mode, charge thresholds, USB charging | Not available | This model's firmware exposes no documented writable channel. The tool refuses rather than guess. |
| Fan control, CPU power limits (PL1/PL2) | Not available on Windows | The only in-system channel is Lenovo's private driver; this project does not use vendor-private paths. On Linux, standard kernel interfaces apply. |
| Vendor-only performance modes (`geek`, custom) | Refused | They have no OS-level equivalent; only mapped modes are offered. |
| BIOS setting writes | Locked | Reads work. Writes stay locked until the firmware's staging behavior is independently verified. |
| Smart sensing, Dolby / noise-cancel audio | Not available | These require Lenovo's licensed service, which is not usable here. |
| BIOS update flashing, BIOS passwords | Out of scope | Deliberately excluded for safety. Driver packages from local `.inf` are supported via `update local-driver`. |

## Safety model

- Read-only commands never write to hardware.
- A write requires `--yes`; `--dry-run` shows the exact intended change first.
- After every write the value is read back; on mismatch the tool rolls back
  and reports the failure.
- It never flashes firmware, never sets or clears BIOS passwords, and never
  weakens VBS/HVCI.

## What it won't do

No bundled vendor binaries, no app store, no accounts, no telemetry, no ads.
`unavailable` in `vantage capability` means "no verified channel on this
machine" — it is a boundary, not a bug to hide.

## Documentation

`docs/` holds the engineering record: interface notes, reverse-engineering
findings, per-capability acceptance evidence (C1–C23), and the Linux backend
mapping. Start with `docs/C-acceptance.md` for what was proven on real
hardware and how.

## Development

Requires Rust 1.85 or newer.

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p vantage
```

CI exercises Linux and Windows. Hardware smoke tests run only on the target
machine, never in CI.

Repository: [cyjin-yl/vantage-cli](https://github.com/cyjin-yl/vantage-cli)

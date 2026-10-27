# lctrl Windows HAL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Establish the Windows hardware abstraction layer transport: WMI method invocation patterns, EnergyDrv IOCTL codecs, Windows error mapping, and capability detection — all testable on Linux via transport-neutral traits, with Windows FFI gated behind `cfg(windows)`.

**Architecture:** Add `lctrl-hal-win` crate. Split into: (1) `transport` module — OS-neutral traits for WMI and IOCTL operations, buffer codecs, and error mapping; (2) `wmi` module — Windows WMI connection and method invocation (cfg-gated); (3) `ioctl` module — EnergyDrv device handle and IOCTL dispatch (cfg-gated); (4) `error` module — Windows error code to `LctrlError` mapping. Transport traits and codecs compile and test on all platforms; FFI implementations compile only on Windows.

**Tech Stack:** Rust 2024; `wmi` crate (Windows only); `windows-sys` crate (Windows only); `serde`/`serde_json` for serialization tests.

**Spec:** `HANDOFF.md`, `docs/01-hal-interfaces.md`, `docs/08-architecture.md`

## Global Constraints

- Implement only from clean-room specification and public APIs; never inspect vendor binaries.
- `lctrl-hal-win` must never depend on `lctrl-hal-linux`; shared logic stays in `lctrl-core`/`lctrl-hal`.
- Every write path must support dry-run, preserve the pre-write value, perform readback, and surface mismatches as exit code 7.
- Exit codes: 0 success, 2 invalid argument, 3 unsupported, 4 channel unavailable, 5 permission denied, 6 firmware rejected, 7 verify mismatch, 1 other.
- Do not expose arbitrary battery-threshold writes or a Windows MSR/RAPL path.
- `LENOVO_GAMEZONE_DATA` methods are permanently unusable on 21VG (`WBEM_E_INVALID_OBJECT`); never call them.
- WMI methods return `Boolean`; success = `true` + `Data == 0`; `false` = firmware rejection.
- EnergyDrv IOCTL input is 1-byte subcommand for GBMD (`0x831020f8`), 4-byte for generic GET/SET.
- `DeviceIoControl` failure: GLE 5 = access denied, 87 = invalid parameter (unsupported cmd), 21 = not ready, 1117 = I/O device error.
- WMI instance methods require binding to an instance `__Path`; static calls fail with "invalid method parameter."
- Read operations are idempotent and may retry; write operations are non-idempent and must read-before-write.

---

### Task 1: Windows error code mapping

**Files:**
- Create: `crates/lctrl-hal-win/Cargo.toml`
- Create: `crates/lctrl-hal-win/src/lib.rs`
- Create: `crates/lctrl-hal-win/src/error.rs`
- Test: `crates/lctrl-hal-win/tests/error_mapping.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: `lctrl_core::{LctrlError, Result}`.
- Produces: `WinError`, `map_win_error(code: u32, context: &str) -> LctrlError`, `map_wmi_hresult(hr: i32, context: &str) -> LctrlError`.

- [ ] **Step 1: Create crate shell and write failing tests**

`crates/lctrl-hal-win/Cargo.toml`:
```toml
[package]
name = "lctrl-hal-win"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
lctrl-core = { path = "../lctrl-core" }

[dev-dependencies]
serde_json.workspace = true
```

Add `crates/lctrl-hal-win` to workspace members.

`crates/lctrl-hal-win/src/lib.rs`:
```rust
mod error;
```

`crates/lctrl-hal-win/src/error.rs` — empty shell.

Write `tests/error_mapping.rs` testing the documented GLE-to-LctrlError mapping:
- GLE 5 (ACCESS_DENIED) → `PermissionDenied { need: "administrator" }`
- GLE 87 (INVALID_PARAMETER) → `Unsupported { feature: context }`
- GLE 21 (NOT_READY) → `ChannelUnavailable { channel: context }`
- GLE 1117 (IO_DEVICE) → `FirmwareRejected { detail: "I/O device error" }`
- GLE 2 (FILE_NOT_FOUND) → `ChannelUnavailable { channel: context }`
- GLE 0 (SUCCESS on failure path) → `Io` error
- HRESULT 0x80041002 (WBEM_E_NOT_FOUND) → `Unsupported`
- HRESULT 0x80041003 (WBEM_E_ACCESS_DENIED) → `PermissionDenied`
- HRESULT 0x80041008 (WBEM_E_INVALID_PARAMETER / "Invalid object" on 21VG) → `Unsupported`
- HRESULT 0x8004100F (WBEM_E_NOT_FOUND class) → `Unsupported`

- [ ] **Step 2: Run test and verify RED**

- [ ] **Step 3: Implement error mapping**

```rust
pub fn map_win_error(gle: u32, context: &str) -> LctrlError { ... }
pub fn map_wmi_hresult(hr: i32, context: &str) -> LctrlError { ... }
```

- [ ] **Step 4: Run test and verify GREEN**

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/lctrl-hal-win
git commit -m "feat(hal-win): add Windows error code mapping"
```

---

### Task 2: EnergyDrv IOCTL buffer codecs

**Files:**
- Modify: `crates/lctrl-hal-win/src/lib.rs`
- Create: `crates/lctrl-hal-win/src/codec.rs`
- Test: `crates/lctrl-hal-win/tests/codec.rs`

**Interfaces:**
- Produces: `GbmdQuery`, `GbmdWrite`, `GenericGet`, `GenericSet`, `BatteryInfo83B` codecs.

- [ ] **Step 1: Write failing codec tests**

Test GBMD query encoding (1-byte input, 4-byte output), generic GET (4-byte cmd input, 4-byte output), generic SET (12-byte `{cmd, p1, p2}`), and 83B battery structure parsing from a known fixture. Test that subcommands `0xFF` (query), `3`/`5` (conservation gen1), `0x0d`/`0x0f` (gen2), `7`/`8` (rapid) encode to the correct single byte.

- [ ] **Step 2: Run and verify RED**

- [ ] **Step 3: Implement codecs**

```rust
pub struct GbmdInput { pub subcmd: u8 }
pub struct GbmdOutput { pub status: u32 }
pub struct GenericGetInput { pub cmd: u32 }
pub struct GenericGetOutput { pub status: u32 }
pub struct GenericSetInput { pub cmd: u32, pub p1: u32, pub p2: u32 }
pub struct AdapterInfo { pub pid: u16, pub vid: u16, pub system_power_w: u16, pub current_power_w: u16 }
pub struct BatteryDetail83B { /* all documented offsets */ }
```

- [ ] **Step 4: Run and verify GREEN**

- [ ] **Step 5: Commit**

---

### Task 3: WMI method result type and invocation contract

**Files:**
- Modify: `crates/lctrl-hal-win/src/lib.rs`
- Create: `crates/lctrl-hal-win/src/wmi_contract.rs`
- Test: `crates/lctrl-hal-win/tests/wmi_contract.rs`

**Interfaces:**
- Produces: `WmiMethodResult`, `check_wmi_method_result(accepted: bool, data: u32, feature: &str) -> Result<u32>`.

- [ ] **Step 1: Write failing tests**

Test that `true + Data==0` → success with value 0; `true + Data!=0` → `FirmwareRejected`; `false` → `FirmwareRejected`; and that the 21VG `Invalid object` HRESULT is never retried.

- [ ] **Step 2-5: Implement, verify, commit**

---

### Task 4: Windows HAL implementation (cfg-gated)

**Files:**
- Create: `crates/lctrl-hal-win/src/wmi.rs` (cfg(windows))
- Create: `crates/lctrl-hal-win/src/ioctl.rs` (cfg(windows))
- Create: `crates/lctrl-hal-win/src/hal.rs` (cfg(windows))
- Modify: `crates/lctrl-hal-win/src/lib.rs`

**Interfaces:**
- Produces: `WinHal` implementing `Hal` + transport traits; `EnergyDrvHandle` with RAII; `WmiConnection` wrapper.

This task compiles only on Windows. On Linux, `cargo check -p lctrl-hal-win` succeeds because the Windows modules are cfg-gated out, leaving only the transport-neutral error/codec/contract modules.

- [ ] **Step 1-5: Implement, verify with `cargo check --workspace`, commit**

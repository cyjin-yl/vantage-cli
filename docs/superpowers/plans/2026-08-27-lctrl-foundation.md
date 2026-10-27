# lctrl Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a compiling Rust workspace with the shared error, capability, and verified-write contracts that every platform backend and CLI command will use.

**Architecture:** Start with only `lctrl-core` and `lctrl-hal`; later phase plans add crates when they have real behavior, avoiding empty placeholder crates. `lctrl-core` owns OS-independent values and serialization. `lctrl-hal` owns the object-safe platform boundary and a synchronous read/write verification algorithm; platform crates will supply concrete channels.

**Tech Stack:** Rust 2024 edition; Cargo resolver 3; `serde`, `serde_json`, and `thiserror`; standard-library traits for HAL boundaries.

**Spec:** `HANDOFF.md`, `docs/00-cleanroom-charter.md`, and `docs/08-architecture.md`

## Global Constraints

- Implement only from this clean-room specification and public APIs; never inspect or distribute vendor binaries or resources.
- Keep `lctrl-core` free of OS APIs; `lctrl-hal-win` and `lctrl-hal-linux` must never depend on each other.
- Every write path must support dry-run, preserve the pre-write value, perform a readback, and surface mismatches as exit code 7.
- Exit codes are fixed: success 0, invalid argument 2, unsupported 3, channel unavailable 4, permission denied 5, firmware rejected 6, verify mismatch 7, all other failures 1.
- JSON errors must have a stable top-level `{"error": {...}}` envelope.
- Do not expose arbitrary battery-threshold writes or a Windows MSR/RAPL path: `HANDOFF.md` §§4–5 and `docs/B-evidence.md` override stale command-tree examples in `docs/08-architecture.md`.
- Do not add daemon, CLI, platform, tuning, IPC, or empty placeholder crates in this phase.

---

### Task 1: Workspace and shared error contract

**Files:**
- Create: `Cargo.toml`
- Create: `crates/lctrl-core/Cargo.toml`
- Create: `crates/lctrl-core/src/lib.rs`
- Create: `crates/lctrl-core/src/error.rs`
- Test: `crates/lctrl-core/tests/error_contract.rs`

**Interfaces:**
- Consumes: the error variants and exit codes in `docs/08-architecture.md` §6.
- Produces: `lctrl_core::{LctrlError, Result, ErrorReport}`; `LctrlError::exit_code() -> u8`; `LctrlError::report() -> ErrorReport`.

- [ ] **Step 1: Create the workspace and an empty core library shell**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/lctrl-core"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.85"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

`crates/lctrl-core/Cargo.toml`:

```toml
[package]
name = "lctrl-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
serde_json.workspace = true
```

`crates/lctrl-core/src/lib.rs` initially contains only:

```rust
mod error;

pub use error::{ErrorReport, LctrlError, Result};
```

Create `crates/lctrl-core/src/error.rs` as an empty file so the RED result is unresolved contract symbols rather than a missing module.

- [ ] **Step 2: Write failing error-contract tests**

Create `crates/lctrl-core/tests/error_contract.rs` with table-driven assertions for all six semantic variants plus the `Io` variant constructed from an `std::io::Error`; every variant must return its fixed exit code. Add JSON assertions that serialization yields top-level key `error`, stable snake-case `kind`, `message`, and the variant-specific context key (`feature`, `channel`, `need`, `detail`, or `requested`/`actual`).

Representative assertions:

```rust
use lctrl_core::LctrlError;

#[test]
fn unsupported_error_has_stable_exit_code_and_json_shape() {
    let error = LctrlError::Unsupported {
        feature: "battery.thresholds".into(),
    };

    assert_eq!(error.exit_code(), 3);
    assert_eq!(
        serde_json::to_value(error.report()).unwrap(),
        serde_json::json!({
            "error": {
                "kind": "unsupported",
                "message": "feature is unsupported: battery.thresholds",
                "feature": "battery.thresholds"
            }
        })
    );
}

#[test]
fn verify_mismatch_reports_both_values() {
    let error = LctrlError::VerifyMismatch {
        requested: "performance".into(),
        actual: "cool".into(),
    };

    assert_eq!(error.exit_code(), 7);
    let value = serde_json::to_value(error.report()).unwrap();
    assert_eq!(value["error"]["requested"], "performance");
    assert_eq!(value["error"]["actual"], "cool");
}
```

- [ ] **Step 3: Run the focused test and verify RED**

Run: `cargo test -p lctrl-core --test error_contract`

Expected: compilation fails because `error.rs` does not yet define the imported public types. This is the intended missing-contract failure, not a manifest or syntax failure.

- [ ] **Step 4: Implement the minimal error model**

Implement `LctrlError` with exactly these variants:

```rust
#[derive(Debug, thiserror::Error)]
pub enum LctrlError {
    #[error("feature is unsupported: {feature}")]
    Unsupported { feature: String },
    #[error("channel is unavailable: {channel}")]
    ChannelUnavailable { channel: String },
    #[error("permission denied; requires {need}")]
    PermissionDenied { need: String },
    #[error("firmware rejected request: {detail}")]
    FirmwareRejected { detail: String },
    #[error("invalid argument: {detail}")]
    InvalidArgument { detail: String },
    #[error("readback mismatch: requested {requested}, actual {actual}")]
    VerifyMismatch { requested: String, actual: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

Define `pub type Result<T> = std::result::Result<T, LctrlError>`. Implement `exit_code()` by exhaustive match. Implement an owned, serializable `ErrorReport { error: ErrorBody }`; `ErrorBody` contains `kind`, `message`, and a flattened `BTreeMap<&'static str, String>` for deterministic context serialization. Do not serialize `std::io::Error` directly; report it as kind `io` with exit code 1 and no extra context.

- [ ] **Step 5: Run the focused test and verify GREEN**

Run: `cargo test -p lctrl-core --test error_contract`

Expected: every error-code and JSON-shape assertion passes.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/lctrl-core
git commit -m "feat(core): define shared error contract"
```

---

### Task 2: Capability model and root HAL boundary

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/lctrl-core/src/lib.rs`
- Create: `crates/lctrl-core/src/capability.rs`
- Test: `crates/lctrl-core/tests/capability_contract.rs`
- Create: `crates/lctrl-hal/Cargo.toml`
- Create: `crates/lctrl-hal/src/lib.rs`
- Test: `crates/lctrl-hal/tests/hal_contract.rs`

**Interfaces:**
- Consumes: `lctrl_core::Result` from Task 1.
- Produces: `Platform`, `Availability`, `Capability`, `CapabilitySet`; `Hal::platform()`, `Hal::hardware_info()`, and `Hal::capabilities()`.

- [ ] **Step 1: Add empty crate shells and failing capability tests**

Add `crates/lctrl-hal` to workspace members. Create `crates/lctrl-hal/Cargo.toml` exactly as follows, and create a `src/lib.rs` shell that re-exports the not-yet-defined HAL symbols so the contract tests fail before behavior exists:

```toml
[package]
name = "lctrl-hal"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
lctrl-core = { path = "../lctrl-core" }
```

In `capability_contract.rs`, assert:

```rust
let mut set = CapabilitySet::new(Platform::Linux);
set.record("battery.status", Availability::Available, None)
    .unwrap();
set.record(
    "tune.pl1",
    Availability::Limited,
    Some("Linux powercap only".into()),
)
.unwrap();

let json = serde_json::to_value(&set).unwrap();
assert_eq!(json["platform"], "linux");
assert_eq!(json["features"]["battery.status"]["availability"], "available");
assert_eq!(json["features"]["tune.pl1"]["detail"], "Linux powercap only");
```

Also assert deterministic key order by serializing a set populated in reverse lexical order and comparing the exact JSON string. Assert that `record` replaces an existing feature rather than creating duplicates. Assert that empty and whitespace-only feature IDs return `LctrlError::InvalidArgument` without changing the set.

In `hal_contract.rs`, create a local fake `Hal` implementation and assert that trait-object calls return platform metadata and capabilities without OS-specific imports.

- [ ] **Step 2: Run both focused tests and verify RED**

Run: `cargo test -p lctrl-core --test capability_contract && cargo test -p lctrl-hal --test hal_contract`

Expected: compilation fails on missing `CapabilitySet` and `Hal` definitions.

- [ ] **Step 3: Implement the capability values**

In `lctrl-core`, define:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform { Windows, Linux }

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability { Available, Limited, Unavailable }

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Capability {
    pub availability: Availability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CapabilitySet {
    pub platform: Platform,
    pub features: std::collections::BTreeMap<String, Capability>,
}
```

`CapabilitySet::record` must reject an empty or whitespace-only feature ID with `LctrlError::InvalidArgument`; otherwise it inserts or replaces the entry. Returning `Result<Option<Capability>>` exposes replacement without a second lookup.

Define OS-neutral metadata:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct HardwareInfo {
    pub product_name: Option<String>,
    pub family: Option<String>,
    pub bios_version: Option<String>,
}
```

- [ ] **Step 4: Implement the object-safe HAL root trait**

In `lctrl-hal`:

```rust
pub trait Hal: Send + Sync {
    fn platform(&self) -> Platform;
    fn hardware_info(&self) -> Result<HardwareInfo>;
    fn capabilities(&self) -> Result<CapabilitySet>;
}
```

No subsystem accessor belongs here yet; each later phase adds one only with its concrete behavior and contract tests.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run: `cargo test -p lctrl-core --test capability_contract && cargo test -p lctrl-hal --test hal_contract`

Expected: both contracts pass with no warnings.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/lctrl-core crates/lctrl-hal
git commit -m "feat(hal): add capability and platform contracts"
```

---

### Task 3: Dry-run and readback verification primitive

**Files:**
- Modify: `crates/lctrl-core/src/lib.rs`
- Create: `crates/lctrl-core/src/change.rs`
- Test: `crates/lctrl-core/tests/change_contract.rs`
- Modify: `crates/lctrl-hal/src/lib.rs`
- Create: `crates/lctrl-hal/src/setting.rs`
- Test: `crates/lctrl-hal/tests/setting_contract.rs`

**Interfaces:**
- Consumes: `lctrl_core::{LctrlError, Result}`.
- Produces: `ApplyMode::{DryRun, Commit}`, `ChangeReport<T>`, object-safe `Setting<T>`, and `apply_setting(&dyn Setting<T>, requested, mode) -> Result<ChangeReport<T>>`.

- [ ] **Step 1: Write failing value-report tests**

`change_contract.rs` must assert exact JSON for dry-run and committed reports:

```rust
let dry_run = ChangeReport::dry_run("normal", "conservation");
assert_eq!(
    serde_json::to_value(dry_run).unwrap(),
    serde_json::json!({
        "mode": "dry_run",
        "previous": "normal",
        "requested": "conservation"
    })
);

let committed = ChangeReport::committed("normal", "conservation", "conservation");
assert_eq!(committed.actual(), Some(&"conservation"));
```

The type must not permit a dry-run report with an `actual` value through its public constructors.

- [ ] **Step 2: Write failing setting-transaction tests**

Use a real in-memory `Setting<String>` with interior mutability and call counters. Cover these observable contracts separately:

1. Dry-run reads once, never writes, never performs a second read, and returns the previous/requested values.
2. Commit reads previous, writes exactly once, reads back exactly once, and returns all three values.
3. A readback mismatch returns `LctrlError::VerifyMismatch` containing display-form requested and actual values.
4. A write error is returned unchanged and suppresses readback.
5. An initial read error suppresses the write.

- [ ] **Step 3: Run focused tests and verify RED**

Run: `cargo test -p lctrl-core --test change_contract && cargo test -p lctrl-hal --test setting_contract`

Expected: compilation fails because the change and setting APIs do not exist.

- [ ] **Step 4: Implement the report and setting transaction**

`ApplyMode` serializes as `dry_run` or `commit`. `ChangeReport<T>` keeps fields private and exposes constructors/accessors. `actual` is omitted during JSON serialization when absent.

Define:

```rust
pub trait Setting<T> {
    fn read(&self) -> Result<T>;
    fn write(&self, value: &T) -> Result<()>;
}

pub fn apply_setting<T>(
    setting: &dyn Setting<T>,
    requested: T,
    mode: ApplyMode,
) -> Result<ChangeReport<T>>
where
    T: Clone + PartialEq + std::fmt::Display,
```

Algorithm: read previous; return `ChangeReport::dry_run` immediately for dry-run; write requested; read actual; compare; return `VerifyMismatch` on inequality; otherwise return committed report. Delays are channel-specific and intentionally remain outside this primitive.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run: `cargo test -p lctrl-core --test change_contract && cargo test -p lctrl-hal --test setting_contract`

Expected: all transaction-order, error-propagation, mismatch, and JSON assertions pass.

- [ ] **Step 6: Run the foundation verification**

Run: `cargo test --workspace`

Expected: all foundation tests and doctests pass with zero failures.

Run: `cargo check --workspace --all-targets`

Expected: clean exit with no warnings.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/lctrl-core crates/lctrl-hal
git commit -m "feat(hal): enforce dry-run and readback verification"
```

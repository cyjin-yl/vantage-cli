# `--dry-run` shadowing: what the source actually does, the single-source fix, and the structural guard

Date: 2026-11-06
Worker: `vantage-re-wave6-1` (source implementation; repair-only mode)
Worktree: `/tank/shared/projects/vantage/replacement/target/bios-docs-20261030` (shared, dirty, ~25 sibling workers; nothing committed or pushed here)
Pre-edit snapshot: `crates/vantage-cli/src/lib.rs` sha256 `8bbad30646665d417d64472fcdfebacf80dcb5b0b33ec9777277d8f8e13aa8c3`
Post-edit state of the same file when this write-up was finished: sha256
`a541c5e10abee861156c165e7df68cb0281f6398164c7ed1bfefdc7b39d58f71`. **Line numbers in §1 are from
the pre-edit read only**; the worktree is shared and kept moving under sibling edits, so every claim
below is also identified by function/variant name, which is the stable anchor.
Parser: clap **4.6.2** / clap_builder **4.6.2** (`Cargo.lock:914-927`). The handed dossier
(`docs/evidence/re-cli-surface-20261105.md` §4.3/§7) cites clap_builder **4.6.7**; this tree builds
4.6.2, the cited code is byte-identical in both, and every quotation below is from the 4.6.2 source
that is actually linked (`~/.cargo/registry/src/index.crates.io-*/clap_builder-4.6.2`).

## 0. Method, and what this document does **not** prove

* No build, test run, formatter, linter, or native program was executed by this worker, and no
  hardware was touched. Per this seat's contract the parent owns execution; this document is a
  source-level proof plus the tests that make the property executable.
* Step (1) of the assignment asked for a live reproduction on 21VG. It is deliberately **not** done
  here, for two independent reasons:
  1. The only observable that distinguishes `ApplyMode::DryRun` from `ApplyMode::Commit` for these
     commands is the report the *service* returns, and the mode is handed to the service **before**
     any validation that could stop a write (`execute_fix_run`, `execute_scene_test`,
     `execute_local_driver` all construct the mode first). A probe that would show `"mode":"commit"`
     is therefore itself a commit probe — exactly what the assignment forbids.
  2. This seat may not run builds or native programs; the parent runs the acceptance checks in §7.
* Consequently the claim is settled here by (a) the parser source, quoted with file:line, and
  (b) regression tests that the parent executes. §2.3 states plainly where the handed claim does
  **not** hold, and §3 lists the defects that do.

## 1. The claim handed to this item

> `fix run`, `scene test` and `update local-driver apply|rollback` each declare their own
> `--dry-run`. clap skips propagating a global arg when the subcommand already has an arg with that
> id, so the global `vantage --dry-run …` never reaches those handlers, and
> `execute_local_driver` builds `ApplyMode::Commit` while the operator believes they asked for a
> preview.

Sites in the pre-edit snapshot (`lib.rs@8bbad306`):

| # | declaration | line | handler that reads it |
|---|---|---|---|
| 1 | `Cli::dry_run` — `#[arg(long, global = true)]` | 423-424 | `execute_with_services` (1672) |
| 2 | `FixCommand::Run { dry_run }` | 645-649 | `execute_fix_run(.., dry_run)` (1933 → 6320) |
| 3 | `SceneCommand::Test { dry_run }` | 660-667 | `execute_scene_test(.., dry_run)` (1944-1953 → 6381) |
| 4 | `LocalDriverCommand::Apply { dry_run }` | 1229-1237 | `execute_local_driver` (6011 → 6098, arm 6106) |
| 5 | `LocalDriverCommand::Rollback { dry_run }` | 1239-1272 | `execute_local_driver` (arm 6120) |
| 6 | `ProfileCommand::Apply { dry_run }` | 874-878 | merged at 4584 (`dry_run \|\| global_apply == DryRun`) |

`execute_with_services` derives its own mode from the global flag plus the `tune` local flag only
(1664-1676):

```rust
let local_dry_run = matches!(
    &cli.command,
    Command::Tune { command: Some(TuneCommand::Profile { command: Some(ProfileCommand::Apply { dry_run: true, .. }) }) }
);
let apply = if cli.dry_run || local_dry_run { ApplyMode::DryRun } else { ApplyMode::Commit };
```

## 2. What the source actually shows

### 2.1 The build-time half of the claim is correct

`Command::_build_self` propagates each global arg into every direct subcommand, and skips a
subcommand that already declares an arg with the same **id** (`command.rs:4743-4770`; `find` compares
`arg.get_id()`, `command.rs:4950-4952`):

```rust
for a in self.args.args().filter(|a| a.is_global_set()) {
    if sc.find(&a.id).is_some() {
        debug!("Command::_propagate skipping {:?} to {}, already exists", a.id, sc.get_name());
        continue;
    }
    sc.args.push(a.clone());
}
```

So for `fix run`, `scene test`, `update local-driver apply`, `update local-driver rollback` and
`tune profile apply` the **clone is not added** and the leaf keeps only its own, non-global
`--dry-run`. The dossier stops here and concludes the value is lost.

### 2.2 The parse-time half contradicts that conclusion: values propagate by id in both directions

clap propagates *matched values* separately from *declarations*, after parsing
(`builder/command.rs:4362-4385`):

```rust
self._build_self(false);
let mut matcher = ArgMatcher::new(self);
… parser.get_matches_with(&mut matcher, raw_args, args_cursor) …
let mut global_arg_vec = Default::default();
self.get_used_global_args(&matcher, &mut global_arg_vec);   // global ids along the *used* path
matcher.propagate_globals(&global_arg_vec);                  // values, by id, at every level
```

`get_used_global_args` (`command.rs:4335-4350`) walks the used-subcommand chain and collects the
global id `dry_run` from the **root** (the leaf's own copy is not global, but the id is already in the
vector because the root declares it global).

`ArgMatcher::propagate_globals` → `fill_in_global_values` (`parser/arg_matcher.rs:47-98`) recursively
walks parent → child, keeps the *higher-precedence* value for each global id, then writes the winner
into **every** level's matches:

```rust
for global_arg in global_arg_vec {
    if let Some(ma) = self.get(global_arg) {
        let to_update = if let Some(parent_ma) = vals_map.get(global_arg) {
            if parent_ma.source() > ma.source() { parent_ma } else { ma }   // precedence, not "child wins"
        } else { ma }.clone();
        vals_map.insert(global_arg.clone(), to_update);
    }
}
… recurse into self.matches.subcommand …
for (name, matched_arg) in vals_map.iter_mut() { self.matches.args.insert(name.clone(), matched_arg.clone()); }
```

Precedence is `ValueSource` ordering (`parser/matches/value_source.rs:1-11`):
`DefaultValue < EnvVariable < CommandLine`. Absent flags get a `DefaultValue` entry from
`Parser::add_defaults` (`parser/parser.rs:53-70`, `1444-1453`), so:

| argv | root `dry_run` | leaf `dry_run` (its own arg) | winner | leaf matches after propagation |
|---|---|---|---|---|
| `vantage --dry-run fix run X` | `true` / CommandLine | `false` / DefaultValue | root | **`true`** |
| `vantage fix run X --dry-run` | `false` / DefaultValue | `true` / CommandLine | leaf | **`true`** |
| `vantage fix run X` | `false` / DefaultValue | `false` / DefaultValue | either | `false` |

The same table applies verbatim to `scene test`, both `update local-driver` leaves and
`tune profile apply`, because propagation is keyed on the **id** `dry_run` and that id is in the
global vector at every level (the root declares it, and `get_used_global_args` re-collects it as the
walk passes each propagated clone).

### 2.3 Consequence — the handed consequence does not follow

* `vantage --dry-run update local-driver apply <inf> <hwid>` reaches
  `LocalDriverCommand::Apply { dry_run: true }`; `execute_local_driver` builds `ApplyMode::DryRun`.
  The same holds for the other four leaves. **The safety consequence in the dossier ("can COMMIT a
  hardware mutation") is not reproducible by this mechanism in clap 4.6.2, and no on-target
  observation of it exists in the record** (§7.6 of the dossier lists that control as *required,
  not yet run*).
* What *is* wrong is the shape, and it is worth fixing because the current behaviour is correct only
  by an undocumented precedence rule applied across an **id collision**:
  1. Five subcommands re-declare an id that the root already declares global. Their correctness
     depends on `parent CommandLine > child DefaultValue` — behaviour clap does not document for
     non-global child args, and which the dossier itself (reading only `_propagate_global_args`)
     got wrong. Any of: renaming the field, adding a second occurrence, making the leaf arg
     `global = true` with a different action, or a clap precedence change silently flips five
     write paths to `Commit`.
  2. `execute_local_driver` never sees the resolved `ApplyMode` at all: it *re-derives* the mode
     from a local bool inside the handler, so the CLI-wide resolution can never be authoritative
     there. `execute_fix_run`/`execute_scene_test` do the same. `update local-install`, `osd`,
     `hotkey` and `tune profile apply` all take the resolved `ApplyMode` instead — the asymmetry is
     what makes a future edit unsafe.
  3. No test in the **pre-edit** tree pinned any of it: every `--dry-run` case in
     `crates/vantage-cli/tests/*.rs` used a leaf that already consumed the resolved `ApplyMode`, and
     none parsed a *root* `--dry-run` for these five leaves or asserted the mode a service receives.
     §5 adds exactly that coverage in a separate test binary.
* Two further consequences that are user-visible but not safety-relevant: the local declarations
  replace the global flag's help description on those leaves (the leaf doc comments say
  "Validate and report without installing." while the global says "Validate a mutating command and
  report the intended change without writing hardware"), and `vantage fix run --help` documents the
  flag twice in the command tree (once per level) instead of once.

## 3. Chosen fix — one authoritative resolution

**Delete the five shadowing fields** (fix run, scene test, local-driver apply, local-driver rollback,
tune profile apply) and thread the already-resolved `ApplyMode` into every handler that used to
re-derive it. Rejected alternative: keep the local flags as aliases that resolve to the same value —
it preserves two sources of truth and two precedence paths, so nothing prevents the next drift; there
is no way to make "two declarations of the same id" un-driftable.

Why deletion cannot break the documented spellings:

* `--dry-run` is declared once, at the root, with `global = true` (`lib.rs:423-424`). §2.2 shows the
  propagated clone exists at every level, so the *trailing* spelling
  `vantage fix run X --dry-run` / `vantage update local-driver apply <inf> <hwid> --dry-run` still
  parses and still yields `Cli::dry_run = true` — no user-facing grammar is removed.
* Help text keeps a `--dry-run` line under each of those leaves (global args are listed in every
  subcommand's `Options:` block; the only filter that hides globals is the *flattened subcommands*
  section, `output/help_template.rs:916`), so README/`docs/08` rows that document `[--dry-run]` on
  these leaves remain accurate. No doc file needs to change for the fix itself.
* Handlers stop computing modes: `execute_fix_run`, `execute_scene_test` and
  `execute_local_driver` take `ApplyMode`, exactly like `execute_local_package`, `execute_osd`,
  `execute_hotkey` and `execute_tune_profile` already do. With the duplicate declarations gone, the
  invariant "exactly one `--dry-run` in the whole tree; every write path consumes the one resolved
  mode" is structural, and §5.4 asserts it on the clap command tree so it cannot drift back.

## 4. Complete survey of global-id collisions (assignment step 4)

Global ids in the pre-edit tree (`lib.rs:418-431`): `json` (id `json`), `dry_run` (long
`--dry-run`), `yes` (id `yes`), `auto_restore` (long `--auto-restore`).
Search performed over `crates/**` and `vantage/**` (excluding `target/`): every `#[arg(long …)]`
declaration, every `*: bool` leaf field, and every explicit `long = "…"` / `name = "…"`.

| colliding id | declarations | verdict |
|---|---|---|
| `dry_run` | `FixCommand::Run`, `SceneCommand::Test`, `LocalDriverCommand::Apply`, `LocalDriverCommand::Rollback`, `ProfileCommand::Apply` | **the five sites of this item; all removed** |
| `json` | root only | no collision — no subcommand declares `json` |
| `yes` | root only | no collision |
| `auto_restore` | root only | no collision (`auto_restore: bool` occurrences in `lib.rs` are function parameters, not clap args; `vantage/src/main.rs` only forwards `cli.dry_run` to the daemon helpers) |

No other subcommand in the tree declares an arg whose id (or long name) matches a global one, so after
this change the whole command tree has zero global-id collisions. `crates/vantage-daemon` and
`crates/vantage-gui` do not define a second clap surface (only `vantage-cli-lib` derives `Cli`).

**Scope of this survey:** it answers "which subcommands collide with a global id", not "which
subcommands honour `--dry-run`". Those are different classes and must not be merged: `radio` is a
concrete counter-example from the same audit window — `RadioCommand` never declared `--dry-run` (so
it is absent from this table) yet `execute_radio` and `RadioControl::radio_set_state` carried no
`ApplyMode` at all until their owner added it, so `vantage --dry-run radio off` wrote. That no-mode
class is covered by `docs/evidence/dryrun-audit-20261106.md` (§2.1 / §3 F5), not here; this item
only guarantees that a dry-run request cannot be *lost* between the CLI root and a handler.

## 5. Regression tests added

New file `crates/vantage-cli/tests/dry_run_resolution.rs` (a new file, so the shared, sibling-edited
`tests/cli_contract.rs` is not touched). Fake `Hal` + recording `MachineFixControl`,
`ScenePolicyControl`, `LocalUpdateControl` fakes; each fake appends the `ApplyMode` it was handed.

1. `root_dry_run_reaches_the_fix_run_handler`, `root_dry_run_reaches_the_scene_test_handler`,
   `root_dry_run_reaches_local_driver_apply`, `root_dry_run_reaches_local_driver_rollback` — parse
   `vantage --dry-run <leaf>`, assert `cli.dry_run`, execute, assert the mode the *service* was
   handed is `DryRun` and `output.json["mode"] == "dry_run"`.
2. `trailing_and_middle_dry_run_still_reach_every_shadowed_handler` — the same four leaves with
   `--dry-run` **after** the leaf (the spelling the deleted local flags used to serve) and at the
   intermediate group levels (`vantage fix --dry-run run`, `vantage scene --dry-run test`,
   `vantage update --dry-run local-driver apply|rollback`, `vantage update local-driver --dry-run
   apply`). Every position must reach the service as `DryRun`. This is the compatibility guard for
   the deletion **and** the guard on the up-propagation path: with the duplicate declarations gone,
   that path is the only thing that carries a trailing flag to `Cli::dry_run`.
3. `commit_without_either_spelling_still_commits` — the four leaves with no flag at all: recorded
   mode must be `Commit`, proving the recorder observes a real resolution rather than a constant.
4. `tune_profile_apply_reads_the_single_global_flag` — the fifth formerly-colliding leaf, parse-level
   only (it needs the real profile catalogue): both spellings must set `cli.dry_run`.
5. `no_subcommand_redeclares_a_global_id` — walks `Cli::command()` (clap `CommandFactory`) and fails
   if any descendant command declares an arg id that the root declares `global`. **This is the test
   that fails on the pre-edit tree** (five collisions) and the one that keeps the class of defect
   from coming back.

Honest statement of old-code behaviour (this seat cannot execute, so this is the §2.2 prediction, to
be confirmed by the parent): tests 1-4 are expected to **pass** on the pre-edit snapshot as well,
because the value did propagate over the id collision; test 5 is expected to **fail** on the
pre-edit snapshot and pass after the change. If tests 1-4 do fail on the old snapshot, the dossier's
claim is right and §2.2's reading is wrong — the fix removes the dependency either way.

## 6. Files changed

* `crates/vantage-cli/src/lib.rs`, exactly these edits (anchors are function/variant names; line
  numbers are from the post-edit sha in the header and move with sibling edits):

  | edit | anchor |
  |---|---|
  | root `--dry-run` is now the only declaration | `Cli::dry_run` (`pub dry_run: bool` at 477) |
  | `FixCommand::Run { issue }` — field removed | enum `FixCommand` |
  | `SceneCommand::Test { event, rule }` — field removed | enum `SceneCommand` |
  | `ProfileCommand::Apply { name }` — field removed | enum `ProfileCommand` |
  | `LocalDriverCommand::Apply { inf, hardware_id }`, `::Rollback { … }` — field removed | enum `LocalDriverCommand` |
  | single resolution, `local_dry_run` deleted | `execute_with_services` (`let apply = if cli.dry_run` at 1819) |
  | `fix run` arm passes `apply` | `execute_with_services` (`FixCommand::Run { issue }` at 2097) |
  | `scene test` arm passes `apply` | `execute_with_services` (`SceneCommand::Test { event, rule }` at 2107) |
  | `execute_local_driver(local_update, mode, command)`; both arms use `mode` | 6672 |
  | `execute_fix_run(.., mode: ApplyMode)`, verb from `mode` | 6993 |
  | `execute_scene_test(.., mode: ApplyMode)`, verb from `mode` | 7053 |
  | `tune profile apply` uses `let mode = global_apply;` | `execute_tune_profile` (5162/5168) |
  | `update` arm passes the resolved mode | `execute_update` (`UpdateCommand::LocalDriver`) |

* `crates/vantage-cli/tests/dry_run_resolution.rs` — new.
* this document.

Not changed on purpose: `README.md`, `docs/08-architecture.md`, `docs/11-sysupdate-diagnostics.md` —
their `[--dry-run]` rows stay true (§3); they are sibling-held and the shared worktree must not carry
a second writer. `crates/vantage-cli/src/capability.rs` already matches these variants with `..`, so
no change there.

## 7. Parent checks and negative controls

1. `cargo test -p vantage-cli-lib --test dry_run_resolution` — the eight tests above. The package is
   `vantage-cli-lib` (`vantage-cli` is the binary package). Build on the target as usual: `vcvars64`
   + toolchain bin on `PATH`, `CARGO_TARGET_DIR=C:\Users\ezra\vantage-target`.
   Expected: all eight pass. The four `root_dry_run_*` tests are the ones that would catch a
   regression to per-handler mode derivation; `no_subcommand_redeclares_a_global_id` is the
   structural guard.
2. `cargo test -p vantage-cli-lib --test cli_contract --test cli_capability --test process_policy` — the
   existing parse/behaviour guards, which exercise `--dry-run` in its global position on many other
   leaves.
3. `cargo check -p vantage-cli-lib -p vantage-cli --all-targets` (Linux host; this is what type-checks
   the new test file) and the normal Windows build, to confirm no other caller of the five variants
   existed (a struct-variant field removal is a compile error everywhere it is constructed;
   `capability.rs` was the only other matcher and it uses `..`).
4. Parse negative controls (no hardware): `vantage update local-driver apply --help` still lists
   `--dry-run`; `vantage fix run --dry-run` (no issue) must exit 2 with a *usage* error, not silently
   plan; `vantage --dry-run fix run no.such.issue` must report dry-run/unavailable and change nothing.
5. Old-behaviour datum, only if the parent wants it measured: the new test file only touches public
   API (`Cli`, `execute_with_services`, `Cli::command()`), so it compiles against the pre-edit
   `lib.rs` too. In a **scratch copy** (never the shared tree), restore
   `git show HEAD:crates/vantage-cli/src/lib.rs`, keep `tests/dry_run_resolution.rs`, and run it.
   §2.2 predicts: `no_subcommand_redeclares_a_global_id` fails with five collisions, everything else
   passes (i.e. the old handlers did receive `DryRun`). If instead a `root_dry_run_*` test fails
   there, the dossier's claim is right and §2.2's reading is wrong — either way the fix stands.
   Do not run the pre-edit binary against a real `.inf`: §0 explains why that probe is a commit.
6. Read back the diff for the five handlers: no `if dry_run` / `mode ==` re-derivation may remain
   outside `execute_with_services` for these leaves.

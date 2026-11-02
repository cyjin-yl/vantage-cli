# Native acceptance 2026-11-03 — process channel, startup-crash root cause, RPC transport faults

Target: TB14P-EZRA (Windows 11 build 26100.9711, session `ezra` console/1). Source tree
`C:\Users\ezra\vantage-machine-fix-source`, synced from the repository worktree
`work/vantage-machine-fix-20261030`. Target build:
`cargo build --release --offline -p vantage`, `CARGO_TARGET_DIR=D:\YanduBuildCache\cargo-target`,
`vantage.exe` SHA-256 `B1ECD1D072E606036FC18765AEEE7EA48E354DC17E10C76EBC9EA8E3F1F2E16F`
(previous build `295B621997679B6328B3EEDB74A5806B8B0951AF48408F4C251109870FF11CB5`).

## 1. Per-process telemetry and policy control (`perf top|boost|throttle`) — accepted

The Windows process controller existed but was never constructed, and the CLI's
`perf top/boost/throttle` fell through to "not attached". It is now wired to the
service graph and exercised on the target against a disposable child process
(`powershell -NoProfile -Command "Start-Sleep -Seconds 900"`), with the priority read
back by an independent observer (`Get-Process -Id <pid> | Select PriorityClass`, a
different process from the writer).

```text
--- perf top : vantage --json perf top
EXIT=0
{"processes":[{"identity":{"creation_time_100ns":134381055223257070,"pid":38484},
"image_name":"ShellHost.exe","image_path":"C:\\Windows\\System32\\ShellHost.exe",
"interval_cpu_time_100ns":1562500,"interval_wall_time_100ns":1864822,
"total_cpu_time_100ns":3013125000,"working_set_bytes":187236352}, ...]}

CHILD_PID=4392
PRIORITY_BEFORE=Normal
--- boost-dry-run : vantage --json --dry-run perf boost 4392
EXIT=0   requested above_normal / actual null / applied false
PRIORITY_AFTER_DRYRUN=Normal            <- dry run left the process untouched
--- boost-commit : vantage --json --yes perf boost 4392
EXIT=0   applied true / actual above_normal
PRIORITY_AFTER_BOOST=AboveNormal        <- independent readback agrees
--- throttle-commit : vantage --json --yes perf throttle 4392
EXIT=0   applied true / actual below_normal
PRIORITY_AFTER_THROTTLE=BelowNormal     <- independent readback agrees
--- boost-auto-restore : vantage --json --yes --auto-restore perf boost 4392
EXIT=0   previous below_normal / actual above_normal
PRIORITY_AFTER_RESTORE=BelowNormal      <- restored to the snapshot taken by this command
```

Accepted: interval CPU telemetry with per-process identity (PID + creation time), a
dry-run that provably performs no write, a real priority write verified by an
independent observer, and `--auto-restore` returning the process to the state that
command captured (the throttle had already lowered it, so the restore target is
`below_normal`, not `normal`).

Not accepted, and deliberately not claimed: thread priority, processor-affinity
selection, working-set trimming, the game whitelist and foreground-window
classification are not implemented — the original's `SetProcessPriorityBoost`,
`SetThreadPriority`, `SetProcessWorkingSetSizeEx` and P/E-core selection have no
public equivalent here, so no approximation is substituted. The first write without
`--yes` is refused by the existing vendor-conflict preflight
(`stop LenovoVantage.exe … or re-run with --yes to accept concurrent-control risk`);
`--yes` is the operator's acceptance of that risk, not an independent verification.

## 2. A startup access violation that killed every command — root cause

Every `vantage` invocation (including `info`) terminated with exit code
`-1073741819` (`0xC0000005`, access violation) in `ntdll.dll`. Application event log:
`Faulting module name: ntdll.dll, Exception code: 0xc0000005`.

Startup tracing (temporary `eprintln!` markers in the target's copy of
`vantage/src/main.rs` and `crates/lctrl-hal-win/src/dolby.rs`, never committed) located
the fault inside the DAX transport's first NDR call, after the binding had been
accepted:

```text
TRACE23:audio
DAXTRACE:A_meta_new_begin
DAXTRACE:B_compose_begin
DAXTRACE:B_compose_call
DAXTRACE:B2_compose_ok
DAXTRACE:C_binding_ok
DAXTRACE:C_binding_ready
DAXTRACE:F_open_ctx_begin      <- NdrClientCall3(opnum 0, open context)
                              <- no G_open_ctx_end: the call never returned
```

A vectored handler sees the fault (`code=0xc0000005 state=1`) but the process still
dies: the marshaller has already corrupted memory before the exception is raised, so
the fault cannot be converted into an error. The Elevoc transport, which wraps its
calls in a process-wide vectored handler, does not fault on the same machine.

Changes made in response:

- The Elevoc login handshake moved to first use (`NativeElevocRpc::load()` binds only;
  `ensure_login()` runs the token exchange on the first query). Service construction
  therefore performs no third-party RPC.
- The DAX transport is **not attached** any more. Its hand-built NDR descriptor faults
  inside the marshaller on the current target build, so `audio dolby` reports
  `unsupported` (exit 3) instead of taking the process down. The transport code and
  its 2026-11-01 probe remain in the tree for the descriptor work; the channel is
  simply not wired into the CLI.
- The HPD RPC transport faults the same way (`vantage sense status` terminated with
  the same `0xC0000005`), so it is **not attached** either. `UnavailableHpdRpc` reports
  the channel as unsupported, and the sense service falls back to the persisted
  registry projection, labelled as such in the output:

```text
--- sense-status : vantage --json sense status
EXIT=0
{"approach_distance":"middle","capability":1,"facial_feature_registered":true,
 "leave_distance":"middle","leave_wait":"middle","lock_on_leave":true,"pause_video":true,
 "privacy_alert":false,"sensor_type":2,"source":"hpd-registry-store","wake_on_approach":true}
```

`source=hpd-registry-store` is the persisted `HKLM\…\SmartSense\Parameters\HPD`
projection, not a live RPC read; the label exists so a degraded read never looks like
a live one. The legacy SmartSense pipe path stays attached.

Two descriptor variants were tested on the target before detaching the channel:
`InterpreterInfo` pointing at the proxy-info block instead of the server-info-shaped
block, and a dual NDR32/NDR64 syntax table with `nCount = 2` (the shape the working
Elevoc transport uses). Both still faulted, so the descriptor difference is not
identified yet; the remaining suspects are the per-interface proc/type format strings
and the context-handle marshalling for opnum 0.

## 3. Charge mode through the CLI (dry-run, commit, restore)

```text
--- charge-mode-dryrun : vantage --json --dry-run battery charge-mode conservation
EXIT=0  {"mode":"dry_run","previous":"rapid","requested":"conservation"}
--- commit : vantage --json --yes battery charge-mode conservation
EXIT=0  {"actual":"conservation","mode":"commit","previous":"rapid","requested":"conservation"}
--- independent readback : vantage --json battery status
EXIT=0  "charge_mode":"conservation"
--- restore : vantage --json --yes battery charge-mode rapid
EXIT=0  {"actual":"rapid","mode":"commit","previous":"conservation","requested":"rapid"}
--- independent readback
EXIT=0  "charge_mode":"rapid"
```

The machine was left in the `rapid` state it had before this session. No vendor
process or service was stopped, and the conflict preflight was never bypassed.

## 4. Official Vantage UI via CDP — attempted, not completed

The packaged Vantage app (`E046963F.LenovoCompanion_10.2608.43.0_x64__k1h2ywk1493x8`,
AUMID `E046963F.LenovoCompanion_k1h2ywk1493x8!App`) was running in session 1 with
no debugger endpoint. To inspect the UI, the app was closed and relaunched with a
debug port:

- The package executable is not directly executable (`Access is denied` from
  `WindowsApps`), so it can only be started through the shell.
- A temporary per-user WebView2 key
  (`HKCU\Software\Microsoft\Edge\WebView2\AdditionalBrowserArguments =
  --remote-debugging-port=9222`) was set, the app was started through
  `shell:AppsFolder\<AUMID>` from an interactive scheduled task, and the key was
  removed again afterwards.
- The app did not stay alive: every activation route (`shell:AppsFolder` via `explorer`,
  via `cmd /c start`, `Start-Process`, the package's protocol handlers) produced a
  process that exited within seconds, with no crash event in the Application log and
  no listener on 9222. The WebView2 profile under the package's `LocalState\UDF` was
  written during one attempt, so the app does start and then exits.

The launch failure is unrelated to our writes: it reproduces identically with the
charge mode in `rapid` (the restored original state) and in `conservation`.

Consequence: **the official Vantage UI was not observed**, so there is still no
evidence that the official application renders the state this CLI writes. The
driver-state acceptance in section 3 and the persisted-state acceptance in section 1
stand on their own; they do not imply anything about the vendor UI. The Vantage UI
host was running before this session and was not running afterwards — the automated
launch routes above could not bring it back, so it needs a manual start from the
Start menu ("Lenovo Vantage"). The addin and service side was never stopped.

## 5. Review findings folded into this change

A read-only review of the HPD transport returned three concrete defects; all three
are fixed here, and the two behavioural ones carry Windows-only regression tests
that were executed on the target (`cargo test -p lctrl-hal-win --test sense_control`
→ 10 passed, 0 failed):

1. The supplemental registry read was fatal and ran *before* the RPC read, so an
   absent `HKLM\…\SmartSense\Parameters\HPD` key aborted the whole sense read and
   kept the RPC channel unreachable. The registry read is now best-effort: the RPC
   read always runs, the legacy fields are merged only when the projection exists,
   and when neither source is available the channel failure is reported instead of
   an empty state. `missing_registry_projection_does_not_block_the_rpc_read`.
2. "The RPC does not advertise this feature" was derived from whether the *value*
   could be decoded, so an advertised setting with an unexpected raw value was
   refused with a false reason. Advertisement is now the op-5 capability bitset
   (`SenseState.advertised`), which the availability check consults; the decoded
   value stays independent. `an_advertised_feature_stays_available_when_its_value_cannot_be_modelled`.
3. Two source comments pointed at `docs/evidence/rpc-transport-faults-20261103.md`,
   which does not exist; they now point at this file.

The review also raised one open question it could not settle from the tree: which
byte of the authored NDR32 tables diverges from the vendor's — the context-handle
entries of the type format string, the per-op proc format records, or the
explicit-handle descriptor that opnum 0 uses (the shape the two faulting transports
share and the working Elevoc transport does not). Settling it needs the vendor
bytes at `LnvSST.dll` rva 0x33742 / `SmartSenseRpcClient.dll` rva 0x18612 diffed
against the authored tables; it is not identified here.

## 6. Scope of this evidence

- Accepted on the target: `perf top`, `perf boost`, `perf throttle`,
  `--auto-restore` (driver-level priority, independently observed), charge-mode
  dry-run/commit/restore (driver state, independently observed), `sense status`
  (persisted projection, explicitly labelled), `battery status`, `audio
  noise-cancel` (Elevoc channel still functional).
- Not accepted: the official Vantage UI state (section 4), the DAX channel and the
  HPD RPC channel (both fault inside the marshaller and are detached), every
  physical effect (charge threshold at the cell, fan curve, panel image quality).

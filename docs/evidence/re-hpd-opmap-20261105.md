# SmartSense HPD endpoint — complete opcode map and read surfacing (2026-11-05)

Target: TB14P-EZRA (ThinkBook 21VG), Windows, HPD RPC device live.
Scope: `ncalrpc:[HumanPresenceDetectionEndpoint]` (the SmartSense HPD settings
interface). Audio is **not** on this endpoint; Dolby (`ncalrpc:[DaxRpcEndpoint]`)
and Elevoc (`ncalrpc:[ElevocRpcEndpoint]`) are the sibling transports and are
documented separately (`docs/evidence/re-audio-write-20261105.md` §1 states
explicitly that the HPD device carries no audio verb).

Method: read-only static analysis of the retained vendor binaries in
`/tmp/hpdre/{svc,rpc}/` (same SHA-256 set as `hpd-rpc-re-20261102.md` §1), plus
the live read evidence already recorded in this repository. **No target
mutation, no service change, no native run in this pass.** All offsets below
are image RVAs in `LnvSST.dll` (1.0.8.5,
`601F9FC413C9886023642D7D8E1F5A1D7EAAFF333AA08460C8CC144CCBAF85D5`).

## 1. Endpoint identity

| Property | Value |
|---|---|
| Protocol sequence / endpoint | `ncalrpc` / `HumanPresenceDetectionEndpoint` (LnvSST.dll rva 0x34f60 / 0x34f70) |
| Interface | `{848A33AC-B5A7-4253-A54F-E0AC20648468}` v1.0 |
| Transfer syntax | NDR32 `8A885D04-1CEB-11C9-9FE8-08002B104860` v2.0 (server also registers NDR64; the owned client offers NDR32 only) |
| Server dispatch count | **20**, i.e. wire ops 0–19 (`RPC_DISPATCH_TABLE` → `DispatchTable` @0x33670; `MIDL_SERVER_INFO` @0x33bd0) |
| Client-only record | op20 `HandleRpcRequest` is a valid client NDR32 record but is **not registered by this server** (dispatch count is 20) |
| Auth | No `RpcBindingSetAuthInfo*` on the client; endpoint DACL + per-call `RpcImpersonateClient`/privilege check on setters. Endpoint DACL has five ACEs (LOCAL, Authenticated Users, `Lenovo.smartSense_k1h2ywk1493x8` capability, Administrators, LocalSystem), `RPC_IF_AUTOLISTEN \| RPC_IF_ALLOW_LOCAL_ONLY`. |
| Return convention | Every primary op returns an **i64 Win32 status** (`FC_HYPER` return slot), not an HRESULT and not an extra `[out]` argument. `0` = success. The managed wrapper throws `Win32Exception((int)status)`. |

Wire opnum = client `nProcNum` = dispatch index. This is **not** export-list
order (verified on both sides in `hpd-rpc-re-20261102.md` §4).

## 2. Complete opcode table (server-accepted ops 0–19)

`ctx` is the context handle returned by op0. "ret" is the i64 Win32 status.

| Op | Procedure | Request shape | Response shape | Live/status |
|---:|---|---|---|---|
| 0 | Initialize | explicit primitive handle_t (the binding) at stack 0; `[out] context_handle*` | writes a 16-byte server context (`LnvSST.dll 0x142ca` alloc 0x10, `0x14319` `mov dword ptr [rbx],0`, `0x14331` stores the object); null out → 87; alloc failure → **14** (ERROR_OUTOFMEMORY, `0x143d6`); success → 0 | **live**: every `sense status` opens this |
| 1 | ClientClose | `[in,out] context_handle*` | clears the caller handle (`0x141de` `mov qword ptr [rbx],0`) and tears down the callback/context; ret status | live (session drop) |
| 2 | ClientStartCallback | `ctx [in]`; `[in] i64` token | no `[out]` data; registers the push callback (helper `0x13820`); ret status | implemented in vendor client; owned client does not register it |
| 3 | ClientStopCallback | `ctx [in]` | no `[out]` data; unregisters (helper `0x13a2c`); ret status | same as op2 |
| 4 | **GetVersion** | `ctx [in]`; `[out] u32*` | `*out = 2` on this server (`0x14039` `mov dword ptr [rbx],2`, then `0x1403f` xor eax,eax); null out → 87 | **read; static value 2; now surfaced (see §5)** |
| 5 | **GetCapability** | `ctx [in]`; `[out] u64*` | 8-byte capability bitset; server fills it via `0xc798`→`0xaba8`. Live mask **`0x7ff`** | **read; live** |
| 6 | **GetAllSetting** (V1 + V2 share this wire op) | `ctx [in]`; 15 × `[out] u32*` + `[out] wchar_t**` | see §3: 12 mandatory scalars, 2 optional scalars, then the wchar count + MIDL-allocated array. Live values recorded in §3.2 | **read; live** |
| 7 | ResetAllSetting | `ctx [in]` | no `[out]` data; resets user-writable settings to service defaults (`0x143f0` → worker `0xacf4`); ret status | **write; live 2026-11-03** (`sense reset`) |
| 8 | SetApproachEnabled | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `Approach` raw 0/1 | **write; CLI `sense wake-on-approach`; live write acceptance not recorded in retained docs** |
| 9 | SetApproachDistance | `ctx [in]`; `[in] u32` 0/1/2 | ret status only; persists `ApproachDistance = arg+1` | write; same caveat as op8 |
| 10 | SetApproachDistanceAutoAdjust | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `AutoAdjust = arg+1` | **write; live 2026-11-03** (arg 0→1 accepted) |
| 11 | SetPresenceLeaveEnabled | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `PresentLeave` raw 0/1 | **write; live 2026-11-03** (lock-on-leave off/on with readback+restore) |
| 12 | SetPresenceLeaveWaitTime | `ctx [in]`; `[in] u32` 0/1/2 | ret status only; persists `LeaveWait = arg+1` | write; enum persistence observed 2026-11-03 (`LeaveWait=1` for arg 0) |
| 13 | SetPresenceLeaveCameraAssist | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `CameraDetect = arg+1` | **write; live 2026-11-03** (arg 0→1 accepted) |
| 14 | SetPresenceLeaveDistance | `ctx [in]`; `[in] u32` 0/1/2 | ret status only; persists `LeaveDistance = arg+1` | write; observed 2026-11-03 (arg 0→1, arg 2→3) |
| 15 | SetPresenceLeaveDistanceAutoAdjust | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `LeaveAutoAdjust = arg+1` | **write; live 2026-11-03** (arg 0→1 accepted) |
| 16 | SetVideoAutoPauseResume | `ctx [in]`; `[in] u32` 0/1 | ret status only; persists `Video = arg+1` (1=off, 2=on) | **write; live 2026-11-03** (values[10] matched both writes) |
| 17 | SetHPDEnabledYogaModeV2 | `ctx [in]`; `[in] u32` yoga flags 0/1/2/4/8 | ret status only | **write rejected: 4317 ERROR_INVALID_DATA for arg ∈ {0,1,2,4,8}; no Yoga persistence key; not implemented** |
| 18 | reserved scalar | `ctx [in]`; `[in] u32` | ret **120** (ERROR_CALL_NOT_IMPLEMENTED, `0x155fd` `mov eax,0x78`); null/unauthorized argument path returns 87; no setting mutation | reserved; not exposed |
| 19 | reserved string | `ctx [in]`; `[in,string] wchar_t*` | ret **120** (`0x15765`); before returning it measures/copies the string (`0x1df80`, `0x12bd8`) and frees its own allocation, but performs no setting mutation; null string → 87 | reserved; not exposed |
| 20 | HandleRpcRequest | client-only: `ctx`; `[in,size_is(length)] byte*`; `[in] u32 length`; `[out,size_is(*reply_len)] byte**`; `[in,out] u32* reply_len` | **void wire return**; not registered by this server (dispatch count 20) | not callable here |

Callback descriptor: the client `MIDL_SERVER_INFO` also carries one callback
record at PS+0x428 (`FC_CALLBACK_HANDLE`, callback op 0, one `[in] i64` token,
no return). It is a push channel on the existing binding, not a second
endpoint; the owned settings client neither registers nor claims a live
presence stream.

Setter prologue shape (ops 8–17) is uniform: `rcx`=ctx, `edx`=u32 argument,
`RpcImpersonateClient`, privilege check (`0x13300`), `RevertToSelf`, then the
per-op worker. Op7 uses the same envelope without the u32. This confirms the
`[in] u32` request shape for every setter and the context-only shape for op7.

## 3. Op6 `GetAllSetting` — the aggregate read

### 3.1 Response layout (server-enforced, from 0x13ad0)

The server NULL-checks **twelve** scalar output pointers before doing anything
(`0x13c30`–`0x13cba`): the three register arguments (rdx, r8, r9) plus the
first nine stack pointers. A NULL among them returns **87**
(ERROR_INVALID_PARAMETER). Outputs 13 and 14 are **optional**: the server
guards them (`0x13d46` / `0x13d52`) and writes only when the pointer is
non-NULL. The last two parameters are:

- `[out] u32*` = wchar element count (`0x13d68`: local count `+1`, stored, and
  the same value is used as the copy length; allocation is `2 * count` bytes at
  `0x13d6d`, so the count includes the terminator);
- `[out] wchar_t**` = MIDL-allocated array (`0x13d74` alloc, `0x13d8c` memset,
  `0x13da4` copy). The server also returns 14 (ERROR_OUTOFMEMORY) if that
  allocation fails (`0x13dab`).

The vendor V1 export passes only the first 12 real outputs and ignores the rest;
the V2 export passes 13 and reads `EnabledYogaMode`. Both call the same wire op,
which is why the 15-scalar + array layout is fixed.

### 3.2 Scalar field map

| Index | Field (managed V2 name + registry cross-check) | Capability bit |
|---:|---|---|
| 0 | Capacity (support value, **not** the op-5 bitset) | — |
| 1 | SensorType (0 Unknown, 1 VL53L1, 2 VL53L5, 4 CVDSP) | 0x1 |
| 2 | ApproachEnabled | 0x2 |
| 3 | ApproachDistance (0/1/2) | 0x4 |
| 4 | ApproachDistanceAutoAdjust | 0x8 |
| 5 | PresenceLeaveEnabled | 0x10 |
| 6 | PresenceLeaveWaitTime (0/1/2) | 0x20 |
| 7 | PresenceLeaveCameraAssist | 0x40 |
| 8 | PresenceLeaveDistance (0/1/2) | 0x80 |
| 9 | PresenceLeaveDistanceAutoAdjust | 0x100 |
| 10 | VideoAutoPauseResumeEnabled | 0x200 |
| 11 | FaceIdRegistered | 0x400 |
| 12 | **EnabledYogaMode** (V2 only; flags 0/1/2/4/8) | **0x800** |
| 13 | undecided (prior doc "p14"); server fills it when non-NULL | unknown |
| 14 | wchar element count (not a setting) | — |
| 15 (16th param) | `wchar_t**` array (content unexamined by vendor clients) | — |

Capability mask recovered from the managed `FeatureCapabilityType` enum:
`SensorType=1, Approach=2, ApproachDistance=4, ApproachDistanceAdjust=8,
PresenceLeave=16, PresenceLeaveWaitTime=32, PresenceLeaveCameraAssist=64,
PresenceLeaveDistance=128, PresenceLeaveDistanceAdjust=256,
VideoAutoPauseResume=512, FaceIdRegistered=1024, YogaMode=2048`.

Live mask on 21VG is **`0x7ff`** — every bit **except YogaMode (0x800)**.

### 3.3 Op4 version semantics (why it is a real read, not a build stamp)

The server returns the constant `2`; the vendor client uses it as a protocol
generation selector:

- `HumanPresenceDetection.GetAllSetting`: `if _version < 2` → `GetHPDAllSetting`
  (12 outputs), else `GetHPDAllSettingV2` (13 outputs).
- `VideoAutoPauseResumeVersion`: `_version >= 1` → V1, else V0.

So op4 is not the driver file version and not the registry `Version` DWORD
(`0x02000000`); it is the live settings-interface generation.

## 4. Read surface: what was already surfaced, what was not

| Read op | Existing consumer | Gap |
|---|---|---|
| op4 GetVersion | `NativeHpdRpc::version()` (public getter) | **no CLI/state consumer at all**: `sense status` omitted it and `hpd_probe` did not print it |
| op5 GetCapability | `SenseState.advertised` (`u64`, full 8 bytes) | none |
| op6 GetAllSetting | typed fields + `SenseState.capability` / `sensor_type` / `facial_feature_registered` | fields 12–13 intentionally unmodelled (see §5.3) |

## 5. Implementation delta (this pass)

### 5.1 Surfaced op4

- `crates/lctrl-core/src/sensing.rs`: `SenseState` gains
  - `interface_version: Option<u32>` — op4 value, `None` when not read;
  - `interface_version_error: Option<String>` — why it is absent on a live RPC
    snapshot. `None` in the registry store and in test fixtures: absence is
    never synthesized.
  - `Display` prints `hpd-version=<n>` when present and
    `hpd-version=unavailable` when only the error is present.
- `crates/lctrl-hal-win/src/sense_rpc.rs`: the live projection reads op4 after
  op5/op6 and converts its result through the new pure `project_state`
  function. **An op4 failure does not fail `sense status`**: the
  presence/approach/video snapshot from ops 5/6 is returned, the version stays
  `None`, and the error text is retained in `interface_version_error`.
- `crates/lctrl-hal-win/src/sense.rs`: the registry projection sets
  `interface_version: None` / `interface_version_error: None`.
- `crates/lctrl-hal-win/examples/hpd_probe.rs`: prints `version: <n>` so op4 can
  be observed read-only in a single probe run.
- `crates/vantage-cli/src/lib.rs` needs no change: `sense status` already
  serializes the whole `SenseState` (`structured_output` at the `Status` arm).

`SenseState` consequently drops `Copy` and keeps `Clone`. It is a snapshot, not
a token, and `interface_version_error` is an owned string; the type's doc
comment records that so a later reader does not "fix" it back by degrading the
diagnostic. Copy-dependent sites were limited to this surface and were audited
by grep over every file that mentions `SenseState` (8 files): the production
change is `apply_feature_to_state` taking `&SenseState` (explicit `(*state)
.clone()`), and the remaining changes are guard-read `.clone()` calls in
`sense_control.rs` / `cli_contract.rs`. No GUI or event-loop consumer exists,
so no per-frame/per-event clone was added. A sibling that later relies on
`Copy` must be reported, not silently patched outside this surface.

### 5.2 Tests added (source-level; not executed by this worker)

`crates/lctrl-hal-win/src/sense_rpc.rs` `#[cfg(test)] mod tests`:

- `op4_version_is_surfaced_without_changing_settings` — `Ok(2)` →
  `interface_version == Some(2)`, error `None`, settings fields unchanged.
- `op4_failure_keeps_the_settings_snapshot_and_reports_the_error` — `Err(...)` →
  version `None`, error text retained, and `wake_on_approach` /
  `lock_on_leave` / `pause_video` still present from op6.

`crates/lctrl-hal-win/tests/sense_control.rs`: all exhaustive `SenseState`
literals carry `interface_version: None` / `interface_version_error: None`
(honest "not read" fixture value, not a synthesized version).

### 5.3 Deliberately not surfaced

- **Op6 index 12 `EnabledYogaMode`**: op5 advertises `0x7ff`, so bit `0x800` is
  clear on this SKU; op17 rejects every tested argument with 4317 and there is
  no Yoga persistence key. Surfacing it would emit an always-absent value, so
  it stays unmodelled.
- **Op6 index 13**: user-visible meaning not recovered; no invented field.
- **Op6 wchar array**: content is unexamined by both vendor clients; the owned
  client frees it as MIDL-owned memory without reading it.
- **Ops 18/19**: reserved; op18 returns 120 and op19 returns 120 after string
  evaluation; no user-facing name or data is recovered.

## 6. Complete SmartSense map — the legacy settings pipe is a different transport

The HPD RPC table above is the **only** settings channel that accepts writes on
this SKU. The old `\\.\pipe\com.lenovo.smartsense.settings` channel
(`SmartSense.exe`, framing `u32 code [+ u32 arg]`, response i32 0=rejected /
nonzero=accepted / −1=exception) has a larger command namespace
(`0x00010101` ENABLE_SST … `0x001E0101` ENABLE_AD_EXTDISP; full table in
`docs/interfaces.md` §13.2) but the live 2026-11-03 probe returned **0
(rejected)** for every tested write, and `SmartSense.exe` 1.0.8.5 recognizes
only this dispatch set:

`0x10201, 0x10401, 0x10701, 0x10801, 0x10802, 0x70101, 0x80101, 0x90101,
0xa0101, 0xb0101, 0xd0101, 0xd0201, 0xe0101, 0x10803, 0x20201, 0x30201`
(with per-command argument-domain checks that reject the probed values).

That channel therefore remains a documented historical map, not a working
write path; HPD writes never fall back to it.

## 7. Verification status (what was and was not observed)

Observed (retained live evidence, this repository):

- op5 `0x7ff` and op6 raw values on 21VG (`hpd-rpc-autoadjust-cameraassist-20261103.md`).
- op7 reset accepted; user-writable fields reset; read-only keys unchanged.
- ops 10/13/15 accepted with `arg+1` persistence; op17 rejected 4317.
- op11 and op16 write → independent registry readback → restore
  (`dax-hpd-rpc-reattach-20261103.md`).
- op6 enum persistence `arg+1` for ops 12/14 (same doc).

Static only in this pass:

- op4 live value `2` (server write at `0x14039`; managed generation gating).
- ops 8/9 exact live write receipts are not present in the retained docs; the
  CLI verbs exist but must not be called "live verified" until recorded.
- ops 18/19 return 120 (server code); no live call was made.
- op0/1/2/3 shapes are read from the dispatch bodies and the two-sided NDR
  tables; only op0/op1 are exercised by every owned read (context open/close).

Not run by this worker (source-only assignment): compile, target build, any
native command, any write.

## 8. Proposed parent acceptance (read-only)

1. Build the source-bound x64 Windows binary and the `hpd_probe` example on the
   target. Run `hpd_probe` with **no arguments** (read-only path): expect
   `version: 2`, `capability: 0x07ff`, and `values[0..14]` matching §3.2. This is
   the first live op4 receipt; a missing/`zero` version must be reported as a
   real failure, not explained away.
2. Run `vantage --json sense status`: expect `source="hpd-rpc"`,
   `interface_version=2`, `advertised=2047`, and **no**
   `interface_version_error`. Presence/approach/video fields must equal the
   independent `Parameters\HPD` registry read for the mapped keys.
3. Negative control (registry path): with the RPC channel unavailable, the
   `source="hpd-registry-store"` snapshot must contain **no**
   `interface_version` and no `interface_version_error`; do not simulate this by
   disabling the HPD driver or SmartSense service on the live machine.
4. Negative control (op4 failure): cannot be induced against the live endpoint
   without damaging it; the two new `project_state` unit tests are the
   controlled evidence that a failed op4 keeps the settings snapshot and
   reports the error.
5. Confirm no write occurred: the status path issues only ops 0, 5, 6, 4, 1
   (open, reads, close); the probe without arguments performs the same reads.

## 9. Known documentation inconsistency (reported, not edited here)

`docs/interfaces.md` §13.1 still states that the HPD RPC channel crashes in the
NDR marshaller (0xC0000005) and is therefore not mounted into the service
graph. That status was superseded on 2026-11-03 by
`dax-hpd-rpc-reattach-20261103.md` (the crash was the guard's misaligned
`CONTEXT`, fixed; `vantage sense status` and the lock-on-leave write/restore
were live-verified afterwards). The §13.1 opcode table is also incomplete
(it omits ops 2/3/7/10/13/15/17/18/19). Separately, §13.2 still says op7
"未在真机写验证,不暴露子命令", while the later section of
`hpd-rpc-autoadjust-cameraassist-20261103.md` records a live op7 reset and the
`sense reset` command that now exists. This evidence document is the complete
map; the interface doc should be reconciled by its owner rather than by a
concurrent edit in this shared worktree.

## 10. Remaining boundaries

- Op6 index 13 and the wchar array content remain semantically undecoded.
- Op18/19 user-facing names remain unknown; their return behavior is proven.
- NDR64 tables remain undecoded; the owned client offers NDR32 only, which the
  server accepts.
- The sensing pipeline (VL53L5 ToF + IR camera) stays in the vendor
  driver/service; this map only covers the settings RPC.

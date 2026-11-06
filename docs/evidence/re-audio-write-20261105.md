# Audio write verbs — Dolby DAX and Elevoc noise-cancel — 2026-11-05

Scope: recover the **Set** verb behind the two audio read surfaces that are
live on TB14P-EZRA (21VG), and record whether a write implementation is
justified. Read-only static analysis of vendor binaries copied off the target;
no target mutation, no service change, no native run in this pass.

## 1. Conclusion

A documented Set verb exists for **both** surfaces, and both are already
implemented and admitted in this repository:

| Surface | Read verb | **Set verb** | Admitted as |
|---|---|---|---|
| Dolby (`audio dolby`) | op5 `GetDolbyEnabled`, op7 `GetActiveProfile` | **op6 `SetDolbyEnabled`**, **op8 `SetActiveProfile`** | `DispatchSupport::Implemented` |
| Noise-cancel (`audio noise-cancel`) | `GetMode{link:"app_uplink"}` | **`SetEffectMode` → `SetMode{link:"uplink"}`** (+ `SetAlgType` prefix) | `DispatchSupport::Implemented` |

The HPD (SmartSense `HumanPresenceDetectionEndpoint`) device carries **no audio
verb**: its opnum table is presence/approach/leave/video settings plus two
reserved ops that return 120 (`docs/evidence/hpd-rpc-re-20261102.md` §4). Audio
writes therefore go to `ncalrpc:[DaxRpcEndpoint]` (Dolby) and
`ncalrpc:[ElevocRpcEndpoint]` (noise-cancel) only.

Consequence for the assignment: there is nothing to implement — the criterion
"write is implemented when a documented Set verb exists" is already satisfied
at `main` (c07c775). Sections 2–5 are the fresh, version-bound proof; §6 maps it
to source; §7 states what remains unproven.

## 2. Binaries (read-only copies, hashes verified on target)

| File | Path on TB14P-EZRA | Size | SHA-256 |
|---|---|---|---|
| `DolbyRpcClient.dll` | `C:\Program Files\WindowsApps\E046963F.LenovoCompanion_10.2608.62.0_x64__k1h2ywk1493x8\LenovoVantage\DolbyRpcClient.dll` | 524192 | `3DA7AC8A58ED093861A945C04C6B32B1771764C4B41288329316AD52FAB4B7A1` |
| same | `…LenovoCompanion_10.2608.43.0_x64__k1h2ywk1493x8\LenovoVantage\DolbyRpcClient.dll` (byte-identical) | 524192 | `3DA7AC8A…FAB4B7A1` |
| `DAX3API.exe` | `C:\Windows\System32\DriverStore\FileRepository\dax3_swc_aposvc.inf_amd64_8c3a31e30ef1e162\DAX3API.exe` (service `DolbyDAXAPI`, `Start=2`) | 2886792 | `4D098667FCB95572B49216F96A1AE3F7F6F2807B50FAA6D15BB3CDE459BC97D8` |
| `ElevocControlSDK_x64.dll` | `C:\ProgramData\Lenovo\Vantage\Addins\SmartNoiseCancelledAddin\1.3.1.97\ElevocControlSDK_x64.dll` | 662536 | `4D6CE4C9AB7AD883A8B6EE8C19133FDBF64343DD92B944B07400D623E937BC9E` |
| `ElevocControlService.exe` | `C:\Windows\System32\ElevocInstallDriver\ElevocControlService.exe` (service `ElevocService`, `Start=2`) | 1257736 | `FBF9D98F61B978BFD9CBFF5AC7CA654CEFC9E8504054CEE900ED89D9008303C7` |

Note: `DAX3API.exe` is **not** in the WindowsApps Vantage payload — it ships in
the `dax3_swc_aposvc` driver-store package and is registered as service
`DolbyDAXAPI`. Historical §12.2 text that located DAX inside WindowsApps
referred to `DolbyRpcClient.dll` only.

Seat copies: `/tmp/audiowrite/bin/` (this host). Target scratch (read-only
recon output, safe to delete): `C:\Users\ezra\audiowrite-files.txt`,
`C:\Users\ezra\audiowrite-recon2.txt`, the two listing scripts
`C:\Users\ezra\audiowrite-recon{,2}.ps1`, and the copied binaries under
`C:\Users\ezra\audiowrite\`.

## 3. Dolby Set verbs — client side (DolbyRpcClient.dll, x64 VA)

Every export is a thunk into a wrapper that issues
`NdrClientCall3(proxy, nProcNum, pReturn=NULL, ctx, arg…)` — `edx` is
`nProcNum`, `r8d` is cleared to NULL return.

| Export | Thunk | Wrapper → proven constants |
|---|---|---|
| `DolbyClient_GetDolbyEnabled` @`0x1800074e0` | `jmp 0x180001270` | wrapper @`0x180001270`: zero-fill `[rsp+0x30]` (u16 out cell), `r9 = [obj+8]` (session ctx), `xor r8d,r8d`, `mov edx,5` |
| `DolbyClient_SetDolbyEnabled` @`0x1800074f0` | `movzx edx,cl` → `call 0x1800012e0`; returns `mov al,1` | wrapper @`0x1800012e0`: `movzx eax,dl` (u8 argument), `mov [rsp+0x20],eax`, `xor r8d,r8d`, **`mov edx,6`** |
| `DolbyClient_GetActiveProfile` @`0x180007510` | `jmp 0x180001310` | wrapper @`0x180001310`: zero-fill `[rsp+0x30]` (u32 out cell), `mov edx,7` |
| `DolbyClient_SetActiveProfile` @`0x180007520` | `mov edx,ecx` → `call 0x180001370`; returns `mov al,1` | wrapper @`0x180001370`: `mov [rsp+0x20],edx` (u32 argument), `mov edx,8` |

So the vendor's Set contract is: **op6 = one value argument, 0/1 enable**, and
**op8 = one `u32` preset index**. Both wrappers return a constant `true`
regardless of RPC outcome — the vendor's own success signal is a lie, which is
why the owned implementation verifies by readback instead of trusting the call.

## 4. Dolby Set verbs — server side (DAX3API.exe)

`RPC_SERVER_INTERFACE` @`0x14020ae00`:

- `Length 0x60`; `InterfaceId 11c6212d-d5f3-4500-ab16-634dc63037be` v1.0;
  `TransferSyntax 8a885d04-1ceb-11c9-9fe8-08002b104860` (NDR32) v2.0.
- `DispatchTable` @`0x14020ade0`, `Count = 99`; `InterpreterInfo` @`0x14020ada0`;
  `Flags 0x06000000` — the NDR64-capable bits. `RPC_IF_AUTOLISTEN |
  RPC_IF_ALLOW_LOCAL_ONLY (0x21)` are **registration** flags passed to
  `RpcServerRegisterIf3`, not descriptor bits, which is why the owned client
  must pin a single NDR v2.0 syntax (`RPC_CLIENT_INTERFACE.Flags = 0`) instead
  of inheriting the vendor's multi-syntax advertisement.

`MIDL_SERVER_INFO` @`0x14020ada0`: `pStubDesc 0x14020ae60`,
`DispatchTable 0x140208f40`, `ProcString 0x1402096b2`,
`FmtStringOffset 0x14020afb0`, `ThunkTable 0`, `pTransferSyntax 0x140209270`,
`nCount 2` (NDR32 + NDR64 syntax infos), `pSyntaxInfo 0x140208e00`.

Format-string offsets start `0x0000, 0x0024, 0x004a, 0x0088, 0x00b4, 0x00e6,
0x0112, 0x013e, 0x016a, 0x0196, 0x01c2, 0x01ee …`. Record prefix is
`u8 handle_type, u8 oi_flags(0x48), u32 rpc_flags(0), u16 proc_num, u16
stack_size`:

| proc_num | format off | stack_size | scalar `NDR_PARAM_OIF` @`+38` (attr, stack, type) |
|---|---|---|---|
| 5 (Get enabled) | `0x00e6` | `0x10` | `50 21 08 00 06 00` |
| 6 (Set enabled) | `0x0112` | `0x10` | `48 00 08 00 06 00` |
| 7 (Get profile) | `0x013e` | `0x10` | `50 21 08 00 08 00` |
| 8 (Set profile) | `0x016a` | `0x10` | `48 00 08 00 08 00` |

Reading: `0x2150` = `IsOut|IsSimpleRef|IsBasetype|ServerAllocSize(1)` (out
scalar), `0x0048` = `IsIn|IsBasetype` (in scalar), stack index `0x0008` (the
slot after the context handle at `0x0000`), and the type field is the format
character — `0x06` FC_SHORT for the enabled flag (ops 5/6), `0x08` FC_LONG for
the profile index (ops 7/8). `stack_size 0x10` on all four means
"context-handle slot + one scalar slot", i.e. exactly one argument beyond the
session context; the leading six bytes of the parameter block are all zero
(`00 00 00 00 00 00`, the void-return slot — [INFERENCE] on the label, the
bytes are observed) and the second record is the `[in]` context handle
(`08 00 00 00 12 00` = `IsIn`, stack `0`, type-table offset `0x12`). The server
therefore has a real, distinct descriptor for op6 and op8 — not reserved slots
(compare op18/op19 on the HPD interface, which exist and return 120).

## 5. Elevoc set-mode — client SDK and service

`ElevocControlSDK_x64.dll` 1.3.1.97 (strings, image offsets):

- Verbs: `SetEffectMode` @`0x79198/0x791d0/0x79208/0x792c0`,
  `GetCurrentMode` @`0x792d0/0x79308`, `SetDownLinkStatus` @`0x79400/0x79448/
  0x79498`, `SetAlgType` @`0x7dd60`, `RPC2_0::CRpcQuery::RpcSetMode`
  @`0x7e660/0x7e690`.
- Mode-name table, in wire order @`0x79250`: `environmental, shared, private,
  special, voiceid, HQRecord, shared_td, private_td` (`Non-AI` @`0x79240` is the
  algorithm-family literal sent by `SetEffectMode` for the `_td` aliases).
- `SetMode` request keys, in writer order @`0x7e6d0`: `request, "SetMode",
  link, mode, CALLER, elevoc_rpc_protocol, "RPC/2.2", token, endpoint`.

`ElevocControlService.exe`: `RPC_SERVER_INTERFACE` @`0x1400fa120` —
`InterfaceId 6f761170-1054-4339-a573-063dd08c7e16` v4.0, NDR32 v2.0,
`Flags 0x06000000`, `DispatchTable.Count = 1` (a single generic opnum-0 stub),
`MIDL_SERVER_INFO.nCount = 2`, endpoint string `ElevocRpcEndpoint` @`0xfe0d8`
(protocol `ncalrpc` @`0xfe0c8`); handler strings `SetMode` @`0xfc190/0xfdc84`,
`GetMode` @`0xfc658`, `SetAlgType` @`0xfb780`.

So noise-cancel has no per-mode opnum: the Set verb is the JSON `SetMode`
method on the shared opnum 0, which is what `noise.rs` emits, and the SDK's
literal key order matches the owned `Envelope` writer byte for byte.

## 6. Implementation mapping (no edit made; verified present at c07c775)

| Requirement | Source |
|---|---|
| op6 / op8 constants | `crates/lctrl-hal-win/src/dolby.rs:213`, `:215` |
| op6 / op8 call sites | `crates/lctrl-hal-win/src/dolby.rs:639` (`NdrClientCall3(… PROC_SET_DOLBY_ENABLED …)`), `:673` (op8) |
| owned descriptors, op5–8 (`out short`, `in short`, `out long`, `in long`) | `crates/lctrl-hal-win/src/ndr/dolby.rs:198-218` (param vocabulary `:128-159`) |
| write orchestration + readback gate | `crates/lctrl-hal-win/src/dolby.rs:115` `set_dolby_profile` (Off ⇒ op6 only; otherwise op6 then op8; `poll_readback_until` 10 × 50 ms, op5+op7 compare) |
| check `0x10`/`0x06`/`0x08` interpretation above | `crates/lctrl-hal-win/src/ndr/dolby.rs:199-218` emits the same attr/type words the server record shows |
| Elevoc Set verb | `crates/lctrl-hal-win/src/noise.rs:1212` `set_effect_mode` (voice-id gate 4, `6→1`/`7→2` remap, domain check, `SetAlgType` prefix, `SetMode{uplink}`) |
| `SetMode` envelope order | `crates/lctrl-hal-win/src/noise.rs:1129` `rpc_set_mode`, `:653` `Envelope` (`request … elevoc_rpc_protocol, RPC/2.2, token, endpoint`) |
| mode table | `crates/lctrl-hal-win/src/noise.rs:151` `MODE_NAMES` = §5 order |
| HAL write entry points | `crates/lctrl-hal-win/src/noise.rs:93` (`poll_readback` after commit), `crates/lctrl-hal-win/src/dolby.rs:115` |
| composition root attaches both | `vantage/src/main.rs:431-437` (Dolby), `:438-444` (Elevoc), `:472-479` (`with_noise_cancel`, `with_audio`) |
| capability rows | `crates/lctrl-hal/src/command_capability.rs:1277` (`AUDIO_DOLBY`), `:1289` (`AUDIO_NOISE_CANCEL`) — both `DispatchSupport::Implemented` |
| read-only probe gate | `crates/lctrl-hal-win/src/windows_hal.rs:944-1006` (`audio.dolby` from op5, `audio.noise-cancel` from `GetCurrentMode`, `channel.audio` join) |
| CLI surfaces | `crates/vantage-cli/src/lib.rs:951` `AudioCommand`, `:2789` `execute_noise_cancel`, `:2836` `execute_dolby`, `:2638`/`:2665` restore helpers |

## 7. Live status (from prior receipts, not re-run in this pass)

- Dolby reads live: op5/op7 (2026-11-01 and 2026-11-03 probes).
- Dolby **writes live**: `audio dolby movie|game` committed and restored, with
  an independent vendor-side read after the first round
  (`docs/evidence/dax-hpd-rpc-reattach-20261103.md`,
  `docs/C-acceptance.md` rows 722/749).
- Noise-cancel reads live and write accepted: `shared` commit + `off` restore,
  independent vendor-SDK readback (`docs/C-acceptance.md` rows 723/750, 859).
- Not proven: physical effect. Config readback (enabled/profile index, mic
  mode) is not evidence that the DSP audio changed; the one owner-confirmed
  audible observation is recorded as such in C-acceptance and is not
  reproducible by receipt.

## 8. Open items (explicit, not silently changed)

1. **Capability axes** (known-contract-narrowing candidate): both rows keep
   `Readback::Unverified` although both implementations poll readback. Parent
   decision 2026-11-05: leave `Unverified` for now — `Required` asserts that the
   readback is proven to return the written value end-to-end, which needs live
   confirmation before the axis is widened. No axis change made.
2. **Elevoc mode 5 `HQRecord`** exists in the wire domain and in `MODE_NAMES`
   but has no `NoiseCancelArg` value; CLI covers raw 0–4 only. Not a Set-verb
   gap, a surface-coverage choice.
3. **Product naming vs wire names**: `NoiseCancellationMode` maps raw 2
   (`private`) to `Single` and raw 3 (`special`) to `Spatial`
   (`crates/lctrl-core/src/audio.rs:79-100`, `docs/04-peripherals.md:395`).
   Wire behaviour is the vendor's; the label mapping is a project decision and
   should be labelled as such in user-facing docs.
4. **Speaker side**: `SetDownLinkStatus` is a documented SDK verb but is not
   implemented; `NoiseCancelState.speaker_enabled` is the observation-only
   `HKCU\…\F4006` mirror (the service does not consume that key, so writing it
   would be a fake fallback).

## 9. Suggested parent checks

1. Read-only admission: `vantage --json info` (or the capability surface) must
   report `audio.dolby` and `audio.noise-cancel` `available` and their commands
   `implemented`, on a session where `DolbyDAXAPI` and `ElevocService` are
   `Running`.
2. Dry-run non-mutation: snapshot `vantage --json audio dolby` state and
   `HKCU\SOFTWARE\Lenovo\SmartEngine\modules\F4006`, run
   `--dry-run` for each surface, re-read both — values must be unchanged.
3. Committed write with independent readback: choose the inverse of the
   captured baseline, commit, then require the readback the implementation
   polls (`GetDolbyEnabled`/`GetActiveProfile`, `GetCurrentMode`) to equal the
   request; restore the exact baseline with `--auto-restore` and re-read.
4. Negative controls: (a) an out-of-domain request must be rejected client-side
   (`NoiseCancelArg` parse error; `set_effect_mode` domain check at
   `noise.rs:1234`); (b) `audio noise-cancel voice-id` on a non-enrolled
   endpoint must fail with `InvalidArgument`, not silently succeed (the 21VG
   seat currently reports VoiceID enrolled, so pick another endpoint or
   record this control as N/A); (c) a dry-run must never emit an RPC write —
   confirm by `CHANNEL_AUDIO` counters or by observing that no state readback
   change occurred in the vendor-side state.
5. Channel-absence honesty: with the DAX endpoint unreachable the CLI must
   report `ChannelUnavailable`/`Unsupported`, never exit 0 (probe path
   `windows_hal.rs:944-1006`; do not stop the vendor service to create this
   condition — use a machine/session where it is already absent, or accept the
   unit tests as the only control).

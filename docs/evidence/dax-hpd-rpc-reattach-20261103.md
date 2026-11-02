# DAX + HPD RPC transports re-attached (2026-11-03)

## Root cause

The `0xC0000005` reported on 2026-11-03 inside the RPC marshaller was **not**
the NDR descriptor tables and **not** the `[out] context_handle` cell size.
It was the vectored-exception guard itself: `RtlCaptureContext` contains an
`fxsave [rcx+0x100]` that requires a 16-byte-aligned `CONTEXT` buffer, but
`windows-sys`'s x64 `CONTEXT` is `#[repr(C)]` without `align(16)`
(the ABI type is `DECLSPEC_ALIGN(16)`). The plain
`MaybeUninit::<CONTEXT>` stack slot landed at an 8-aligned address and the
guard crashed inside `ntdll!RtlCaptureContext` **before the guarded call
ever ran**. Whether it crashed depended on the caller's stack layout, which
is why the same build appeared to work on 2026-11-01.

Diagnostic path on the target (`dax_probe.exe`, instrumented):
`main: start` → binding composed → `before_op0` →
`EXC 0xC0000005 rip=ntdll+0x124090` (the `fxsave`), `excinfo op=0
data=0xffff_ffff_ffff_ffff`, rcx 8-aligned — never `after_op0`.

## Fix

- `dolby.rs` / `sense_rpc.rs`: wrap the guard's `CONTEXT` in
  `#[repr(C, align(16))]` before `RtlCaptureContext`.
- `vantage/src/main.rs`: attach `WindowsDolbyAudio::new(NativeDolbyRpc::load())`
  and `NativeHpdRpc::new()`; remove the detached-comment stub.
- `sense_rpc.rs`: drop `UnavailableHpdRpc` (dead once the channel is live).
- Context cells widened to 128 B as a defensive floor; `[out]
  context_handle` returns a pointer, so size is not load-bearing.

## Target verification (TB14P-EZRA, elevated, release build)

| Command | Result |
|---|---|
| `dax_probe.exe` | `connect: ok`; op5 enabled=false, op7 profile=5, op2 spatial=true, op37 sku=1 |
| `vantage sense status` | `lock-on-leave=on wake-on-approach=on pause-video=off source=hpd-rpc` |
| `vantage sense lock-on-leave off --yes --auto-restore` | applied, readback `=off`, restored `=on` |
| `vantage audio dolby movie --yes --auto-restore` | `enabled=true profile=Movie`, restored `enabled=false profile=Dynamic` |

All reads and writes exercised live against `ncalrpc:[DaxRpcEndpoint]`
(DAX3API) and the SmartSense HPD endpoint; no Lenovo-shipped binary is
loaded.

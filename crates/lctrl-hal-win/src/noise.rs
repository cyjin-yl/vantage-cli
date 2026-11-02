//! Noise-cancellation control through a pure-Rust `ncalrpc` client for the
//! Elevoc control service.
//!
//! Contract recovered from ElevocControlSDK_x64.dll 1.0.1.1108.8f92a86 and
//! ElevocControlService.exe 1.0.1.1142 on TB14P-EZRA; see
//! `docs/interfaces.md` §14. The vendor SDK forwards to the service over
//! ALPC (`ncalrpc:[ElevocRpcEndpoint]`, interface
//! `{6F761170-1054-4339-A573-063DD08C7E16}` v4.0, NDR32 transfer syntax) using
//! a JSON request envelope on a single NDR opnum-0 stub. This module
//! reimplements that exact stack — bind, authorized-token login, warm-up
//! queries, and the per-method request/response JSON — against the Windows
//! system `rpcrt4.dll` only; no Lenovo-delivered binary is loaded. The DSP
//! pipeline stays inside ElevocControlService — this surface only selects
//! modes.
//!
//! Mode domain (`SetEffectMode`, proven from SDK string slots
//!:
//!
//! | mode | name | notes |
//! |------|---------------|----------------------------------------------|
//! | 0 | environmental | service default; closest to "off" |
//! | 1 | shared | |
//! | 2 | private | |
//! | 3 | special | |
//! | 4 | voiceid | rejected unless VoiceID is enrolled |
//! | 5 | HQRecord | |
//! | 6/7 | shared_td / private_td | legacy aliases remapped to 1/2 by the SDK |
//!
//! `SetEffectMode` results (proven:
//! 1 = success, -1 = RPC/default-endpoint failure, 2 = mode not supported
//! by the endpoint ability list, 4 = VoiceID requested but not enrolled.

use std::ffi::c_void;
use std::time::Duration;

use lctrl_core::{
    ApplyMode, ChangeReport, LctrlError, NoiseCancelState, NoiseCancellationMode, Result,
};
use lctrl_hal::NoiseCancelControl;
use lctrl_hal::poll_readback;

/// Sealed seam over the service wire protocol so [`WindowsNoiseCancel`] stays
/// testable without a running ElevocControlService.
pub trait ElevocRpc: Send + Sync {
    /// `GetCurrentMode(NULL)` — SDK domain 0–7, negative on failure.
    fn current_mode(&self) -> Result<i32>;
    /// `SetEffectMode(mode, NULL)`.
    fn set_effect_mode(&self, mode: u32) -> Result<()>;
    /// `QueryVoiceIDStatus()` — 0 = unset, 2 = enrolled.
    fn voice_id_status(&self) -> Result<i32>;
}

/// `AudioControl`-adjacent noise-cancellation control backed by an
/// [`ElevocRpc`] transport plus the F4006 registry mirror for the
/// speaker-side observation.
pub struct WindowsNoiseCancel<R: ElevocRpc> {
    rpc: R,
}

impl<R: ElevocRpc> WindowsNoiseCancel<R> {
    #[must_use]
    pub const fn new(rpc: R) -> Self {
        Self { rpc }
    }

    fn read_state(&self) -> Result<NoiseCancelState> {
        let mic_mode = match self.rpc.current_mode()? {
            raw @ 0..=7 => NoiseCancellationMode::from_raw(raw as u8),
            raw => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("Elevoc GetCurrentMode returned unexpected value {raw}"),
                });
            }
        };
        let voice_id_enrolled = match self.rpc.voice_id_status() {
            Ok(0) => Some(false),
            Ok(2) => Some(true),
            _ => None,
        };
        Ok(NoiseCancelState {
            mic_mode,
            speaker_enabled: read_f4006_speaker_mirror(),
            voice_id_enrolled,
        })
    }
}

impl<R: ElevocRpc> NoiseCancelControl for WindowsNoiseCancel<R> {
    fn noise_cancel_state(&self) -> Result<NoiseCancelState> {
        self.read_state()
    }

    fn set_noise_cancel_mode(
        &self,
        mode: NoiseCancellationMode,
        apply: ApplyMode,
    ) -> Result<ChangeReport<NoiseCancelState>> {
        let previous = self.read_state()?;
        let mut requested = previous;
        requested.mic_mode = mode;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.rpc.set_effect_mode(u32::from(mode.raw()))?;
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

// ---------------------------------------------------------------------------
// Wire contract constants (docs/interfaces.md §14)
// ---------------------------------------------------------------------------

/// Interface GUID `{6F761170-1054-4339-A573-063DD08C7E16}` v4.0 — the ABI
/// anchor the service registers via `RpcServerRegisterIf3`. Server and client
/// share identical bytes; negotiation fails if the server registers a
/// different version, which is exactly the ABI-stability guarantee relied on.
const ELEVOC_IF_GUID: [u8; 16] = [
    0x70, 0x11, 0x76, 0x6f, 0x54, 0x10, 0x39, 0x43, 0xa5, 0x73, 0x06, 0x3d, 0xd0, 0x8c, 0x7e, 0x16,
];
/// NDR32 transfer syntax `{8A885D04-1CEB-11C9-9FE8-08002B104860}` v2.0.
const NDR32_SYNTAX_GUID: [u8; 16] = [
    0x04, 0x5d, 0x88, 0x8a, 0xeb, 0x1c, 0xc9, 0x11, 0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60,
];
/// NDR64 transfer syntax `{71715533-BABE-4983-19B5-DBEF9CCC36}` v1.0 —
/// the vendor client advertises both syntaxes (`nCount = 2`); the service
/// only offers NDR32, so negotiation always selects the NDR32 record.
const NDR64_SYNTAX_GUID: [u8; 16] = [
    0x33, 0x71, 0x15, 0x71, 0xbe, 0xba, 0x49, 0x83, 0x19, 0xb5, 0xdb, 0xef, 0x9c, 0xcc, 0x36, 0x77,
];
/// NDR64 transfer syntax `{71715533-BABE-4983-19B5-DBEF9CCC36}` v1.0 — the
/// vendor stub lists it as a second syntax entry, but the service only
/// advertises NDR32, so we only offer NDR32 too.
const ENDPOINT_NAME: &[u16] = &[
    0x45, 0x6c, 0x65, 0x76, 0x6f, 0x63, 0x52, 0x70, 0x63, 0x45, 0x6e, 0x64, 0x70, 0x6f, 0x69, 0x6e,
    0x74, 0x00,
];
const PROTO_SEQ: &[u16] = &[0x6e, 0x63, 0x61, 0x6c, 0x72, 0x70, 0x63, 0x00];

// On the observed service version, both an empty and an absent `user` field
// worked in GetAuthorizedTokens. This does not establish that other values
// are accepted: a previously tested non-whitelisted value faulted the service
// handler. Send an empty value and do not retain the original application key.

/// Canonical uplink mode names (SDK table /.
/// The vendor `RpcGetMode` maps `Content.mode` by comparing against these
/// names only (shared→1 … HQRecord→5); anything else, including the
/// `*_td` client-side alias names, falls through to 0 (environmental).
const MODE_NAMES: [&str; 6] = [
    "environmental",
    "shared",
    "private",
    "special",
    "voiceid",
    "HQRecord",
];

/// Preserve the existing outgoing request limit, including its terminator.
const RPC_REQUEST_CHARS: usize = 0x400;
/// The generated sized string allows this many UTF-16 units in a reply.
/// rpcrt4 writes into the caller's storage; the request limit is not a
/// safe allocation bound for that in/out parameter.
const RPC_BUFFER_CHARS: usize = crate::ndr::elevoc::ELEVOC_WIRE_BOUND_WCHARS as usize;

// ---------------------------------------------------------------------------
// MIDL stub metadata. The proc/type format strings are *generated* by
// `crate::ndr` from the recovered signature — `void opnum0([in,out,string]
// wchar_t *json)`, request JSON out and response back in the same UTF-16
// buffer — per the public NDR format-string grammar; nothing is
// transcribed from the vendor stub. `ProxyTables` owns the generated
// buffers so the pointers below stay stable.
// ---------------------------------------------------------------------------

type RpcStatus = i32;
type RpcHandle = *mut c_void;
type RpcAllocFunc = unsafe extern "C" fn(usize) -> *mut c_void;
type RpcFreeFunc = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
struct RpcVersion {
    major_version: u16,
    minor_version: u16,
}

#[repr(C, align(4))]
struct RpcSyntaxIdentifier {
    syntax_guid: [u8; 16],
    syntax_version: RpcVersion,
}

#[repr(C)]
struct RpcClientInterface {
    length: u32,
    interface_id: RpcSyntaxIdentifier,
    transfer_syntax: RpcSyntaxIdentifier,
    dispatch_table: *const c_void,
    rpc_protseq_endpoint_count: u32,
    _pad0: u32,
    rpc_protseq_endpoint: *const c_void,
    reserved: usize,
    interpreter_info: *const c_void,
    flags: u32,
    _pad1: u32,
}

#[repr(C)]
struct MidlSyntaxInfo {
    transfer_syntax: RpcSyntaxIdentifier,
    _pad0: u32,
    dispatch_table: *const c_void,
    proc_string: *const u8,
    fmt_string_offset: *const i16,
    type_string: *const u8,
    reserved: usize,
    reserved2: usize,
    reserved3: usize,
}

#[repr(C)]
struct MidlStublessProxyInfo {
    p_stub_desc: *const MidlStubDesc,
    proc_format_string: *const u8,
    format_string_offset: *const i16,
    p_transfer_syntax: *const RpcSyntaxIdentifier,
    n_count: usize,
    p_syntax_info: *const MidlSyntaxInfo,
}

#[repr(C)]
struct MidlStubDesc {
    rpc_interface_information: *const RpcClientInterface,
    pfn_allocate: RpcAllocFunc,
    pfn_free: RpcFreeFunc,
    p_handle: *const RpcHandle,
    apfn_ndr_rundown_routines: usize,
    a_generic_binding_routine_pairs: usize,
    apfn_expr_eval: usize,
    a_xmit_quintuple: usize,
    p_format_types: *const u8,
    f_check_bounds: i32,
    version: u32,
    p_malloc_free_struct: usize,
    midl_version: i32,
    comm_fault_offsets: usize,
    a_user_marshal_quadruple: usize,
    notify_routine_table: usize,
    m_flags: usize,
    cs_routine_tables: usize,
    proxy_server_info: usize,
    p_expr_info: usize,
}

#[link(name = "rpcrt4")]
unsafe extern "C" {
    fn RpcStringBindingComposeW(
        obj_uuid: *const u16,
        prot_seq: *const u16,
        network_addr: *const u16,
        endpoint: *const u16,
        options: *const u16,
        string_binding: *mut *mut u16,
    ) -> RpcStatus;
    fn RpcBindingFromStringBindingW(
        string_binding: *const u16,
        binding: *mut RpcHandle,
    ) -> RpcStatus;
    fn RpcStringFreeW(string: *mut *mut u16) -> RpcStatus;
    fn RpcBindingFree(binding: *mut RpcHandle) -> RpcStatus;
}
// `NdrClientCall3` has no Rust-typed declaration: it is called only from the
// `elevoc_rpc_invoke_wrapper` assembly via `__imp_NdrClientCall3` (the import
// link above keeps `rpcrt4.lib` on the link line).

#[link(name = "ole32")]
unsafe extern "C" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const [u8; 16],
        outer: *mut c_void,
        clsctx: u32,
        iid: *const [u8; 16],
        out: *mut *mut c_void,
    ) -> i32;
    fn CoTaskMemFree(pv: *mut c_void);
}

#[link(name = "kernel32")]
unsafe extern "C" {
    fn GetProcessHeap() -> *mut c_void;
    fn HeapAlloc(heap: *mut c_void, flags: u32, bytes: usize) -> *mut c_void;
    fn HeapFree(heap: *mut c_void, flags: u32, mem: *mut c_void) -> i32;
}

// ---------------------------------------------------------------------------
// RpcExcept parity: the vendor `SafeRpcRequest` wraps
// `NdrClientCall3` in `__try/__except` and converts RPC exceptions into a
// return code (e.g. `RPC_S_SERVER_UNAVAILABLE` when the service dies
// mid-call). Rust cannot catch SEH exceptions; we reproduce the semantics
// with a vectored exception handler that redirects the faulted call into a
// landing pad which returns to the Rust call site.
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_POINTERS,
};

const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

// `NdrClientCall3` wrapper + landing pad. The wrapper stores its frame
// pointer (`rsp` at the call site) into `*save_slot`; on fault the VEH
// restores that `rsp` and redirects `rip` to the landing pad, whose
// `pop rbp; ret` unwinds exactly the wrapper frame and returns to the
// Rust `call` site — the recovery half of the vendor's `__except`.
core::arch::global_asm!(
    r#"
.globl elevoc_rpc_invoke_wrapper
.def elevoc_rpc_invoke_wrapper; .scl 2; .type 32; .endef
.seh_proc elevoc_rpc_invoke_wrapper
elevoc_rpc_invoke_wrapper:
    push rbp
    .seh_pushreg rbp
    mov rbp, rsp
    .seh_setframe rbp, 0
    sub rsp, 0x30
    .seh_stackalloc 0x30
    .seh_endprologue
    mov [rcx], rbp
    mov rax, [rbp + 0x30]
    mov rcx, rdx
    mov rdx, r8
    mov r8, r9
    mov r9, rax
    call qword ptr [rip + __imp_NdrClientCall3]
    mov rsp, rbp
    pop rbp
    ret
    .seh_endproc

.globl elevoc_rpc_fault_landing
.def elevoc_rpc_fault_landing; .scl 2; .type 32; .endef
.seh_proc elevoc_rpc_fault_landing
elevoc_rpc_fault_landing:
    pop rbp
    ret
    .seh_endproc
"#
);

unsafe extern "C" {
    fn elevoc_rpc_invoke_wrapper(
        save_slot: *mut u64,
        proxy_info: *const MidlStublessProxyInfo,
        proc_num: u32,
        return_ptr: *mut c_void,
        arg0: *mut u16,
    );
    fn elevoc_rpc_fault_landing();
}

thread_local! {
    /// Wrapper's `rsp` at the `call NdrClientCall3` site (its `rbp` value —
    /// `[rbp+8]` is the return address into Rust after `call wrapper`).
    static RPC_FAULT_RSP: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// SEH exception code captured by the VEH for the in-flight call.
    static RPC_FAULT_CODE: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Non-zero while a guarded `NdrClientCall3` is in flight (process-wide;
/// calls are serialized by [`RPC_CALL_LOCK`]).
static RPC_IN_FLIGHT: AtomicU64 = AtomicU64::new(0);

unsafe extern "system" fn elevoc_rpc_veh(info: *mut EXCEPTION_POINTERS) -> i32 {
    unsafe {
        if info.is_null() || RPC_IN_FLIGHT.load(Ordering::Acquire) == 0 {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let saved_rsp = RPC_FAULT_RSP.with(|c| c.get());
        if saved_rsp == 0 {
            // The exception belongs to some other thread.
            return EXCEPTION_CONTINUE_SEARCH;
        }
        let ctx = (*info).ContextRecord;
        let rec = (*info).ExceptionRecord;
        if ctx.is_null() || rec.is_null() {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        RPC_FAULT_CODE.with(|fc| fc.set((*rec).ExceptionCode));
        (*ctx).Rsp = saved_rsp;
        (*ctx).Rip = elevoc_rpc_fault_landing as *const () as usize as u64;
        EXCEPTION_CONTINUE_EXECUTION
    }
}

/// Serialize guarded calls process-wide so an in-flight exception is
/// unambiguously ours before the VEH eats it (mirrors the vendor's
/// serialized `SafeRpcRequest` path).
static RPC_CALL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Install the VEH once per process (idempotent).
fn install_rpc_veh() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = AddVectoredExceptionHandler(1, Some(elevoc_rpc_veh));
    });
}

/// `SafeRpcRequest` equivalent: run one opnum-0 call under the VEH;
/// returns `Err(rpc_status)` for exceptions exactly like the vendor's
/// `__except` returning the exception code.
fn rpc_call_guarded(tables: &ProxyTables, buf: &mut [u16; RPC_BUFFER_CHARS]) -> Result<()> {
    install_rpc_veh();
    let _guard = RPC_CALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    RPC_FAULT_RSP.with(|c| c.set(0));
    RPC_FAULT_CODE.with(|c| c.set(0));
    RPC_IN_FLIGHT.store(1, Ordering::Release);
    unsafe {
        elevoc_rpc_invoke_wrapper(
            RPC_FAULT_RSP.with(|c| c.as_ptr()),
            &raw const tables.proxy_info,
            0,
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
        );
    }
    RPC_IN_FLIGHT.store(0, Ordering::Release);
    let code = RPC_FAULT_CODE.with(|c| c.get());
    RPC_FAULT_RSP.with(|c| c.set(0));
    if code == 0 {
        Ok(())
    } else {
        Err(LctrlError::ChannelUnavailable {
            channel: format!("Elevoc RPC call faulted (exception 0x{code:08x})"),
        })
    }
}

unsafe extern "C" fn midl_alloc(size: usize) -> *mut c_void {
    unsafe { HeapAlloc(GetProcessHeap(), 0, size.max(1)) }
}

unsafe extern "C" fn midl_free(ptr: *mut c_void) {
    if !ptr.is_null() {
        unsafe {
            HeapFree(GetProcessHeap(), 0, ptr);
        }
    }
}

/// Everything `NdrClientCall3` dereferences, boxed so every interior pointer
/// stays stable while the [`NativeElevocRpc`] owner lives.
///
/// The vendor ifspec is dual client+server (its `DispatchTable` points at an
/// `NdrServerCall2` thunk for `RegCallback` and `InterpreterInfo` at a
/// `MIDL_SERVER_INFO`); both fields are dead on the client call path, so the
/// client-side replica leaves `DispatchTable` null and points
/// `InterpreterInfo` at its own `MIDL_STUBLESS_PROXY_INFO` (standard client
/// MIDL convention).
struct ProxyTables {
    handle: RpcHandle,
    /// Generated NDR proc/offset/type tables (`crate::ndr`); every pointer
    /// below must land inside these buffers.
    ndr: crate::ndr::elevoc::NdrDescriptors,
    stub_desc: MidlStubDesc,
    /// Standalone `RPC_SYNTAX_IDENTIFIER` the proxy advertises
    /// (`MIDL_STUBLESS_PROXY_INFO::pTransferSyntax`); the vendor keeps it
    /// separate from `MIDL_SYNTAX_INFO::TransferSyntax`.
    transfer_syntax: RpcSyntaxIdentifier,
    syntax_info: MidlSyntaxInfo,
    /// Second `MIDL_SYNTAX_INFO` record (NDR64); the two records are
    /// adjacent so `pSyntaxInfo` indexes them as an array.
    syntax_info2: MidlSyntaxInfo,
    proxy_info: MidlStublessProxyInfo,
    if_spec: RpcClientInterface,
}

// The table is read-only after construction except `handle`, which the RPC
// runtime treats as its binding slot.
unsafe impl Send for ProxyTables {}
unsafe impl Sync for ProxyTables {}

impl ProxyTables {
    fn new() -> Box<Self> {
        let mut tables = Box::new(ProxyTables {
            handle: std::ptr::null_mut(),
            ndr: crate::ndr::elevoc::generate(&crate::ndr::elevoc::elevoc_stub_signature()),
            stub_desc: MidlStubDesc {
                rpc_interface_information: std::ptr::null(),
                pfn_allocate: midl_alloc,
                pfn_free: midl_free,
                p_handle: std::ptr::null(),
                apfn_ndr_rundown_routines: 0,
                a_generic_binding_routine_pairs: 0,
                apfn_expr_eval: 0,
                a_xmit_quintuple: 0,
                p_format_types: std::ptr::null(),
                f_check_bounds: 1,
                version: 0x0006_0001,
                p_malloc_free_struct: 0,
                midl_version: 0x0801_026e,
                comm_fault_offsets: 0,
                a_user_marshal_quadruple: 0,
                notify_routine_table: 0,
                // MIDL stub-descriptor flags: bit0 selects the -Oicf
                // format-string contract our generator emits; bit25 is the
                // NDR64-metadata-present marker MIDL stamps on dual-syntax
                // stub descriptors (the NDR64 record exists in
                // `syntax_info2` so the marker stays set).
                m_flags: 0x0200_0001,
                cs_routine_tables: 0,
                // Standard client convention: &ProxyInfo (self-referential;
                // patched below).
                proxy_server_info: 0,
                p_expr_info: 0,
            },
            syntax_info: MidlSyntaxInfo {
                transfer_syntax: RpcSyntaxIdentifier {
                    syntax_guid: NDR32_SYNTAX_GUID,
                    syntax_version: RpcVersion {
                        major_version: 2,
                        minor_version: 0,
                    },
                },
                _pad0: 0,
                dispatch_table: std::ptr::null(), // client SyntaxInfo[0]: 0
                proc_string: std::ptr::null(),
                fmt_string_offset: std::ptr::null(),
                type_string: std::ptr::null(),
                reserved: 0,
                reserved2: 0,
                reserved3: 0,
            },
            syntax_info2: MidlSyntaxInfo {
                transfer_syntax: RpcSyntaxIdentifier {
                    syntax_guid: NDR64_SYNTAX_GUID,
                    syntax_version: RpcVersion {
                        major_version: 1,
                        minor_version: 0,
                    },
                },
                _pad0: 0,
                dispatch_table: std::ptr::null(),
                proc_string: std::ptr::null(),
                fmt_string_offset: std::ptr::null(),
                type_string: std::ptr::null(),
                reserved: 0,
                reserved2: 0,
                reserved3: 0,
            },
            transfer_syntax: RpcSyntaxIdentifier {
                syntax_guid: NDR32_SYNTAX_GUID,
                syntax_version: RpcVersion {
                    major_version: 2,
                    minor_version: 0,
                },
            },
            proxy_info: MidlStublessProxyInfo {
                p_stub_desc: std::ptr::null(),
                proc_format_string: std::ptr::null(),
                format_string_offset: std::ptr::null(),
                p_transfer_syntax: std::ptr::null(),
                n_count: 2,
                p_syntax_info: std::ptr::null(),
            },
            if_spec: RpcClientInterface {
                length: 0x60,
                interface_id: RpcSyntaxIdentifier {
                    syntax_guid: ELEVOC_IF_GUID,
                    syntax_version: RpcVersion {
                        major_version: 4,
                        minor_version: 0,
                    },
                },
                transfer_syntax: RpcSyntaxIdentifier {
                    syntax_guid: NDR32_SYNTAX_GUID,
                    syntax_version: RpcVersion {
                        major_version: 2,
                        minor_version: 0,
                    },
                },
                dispatch_table: std::ptr::null(),
                rpc_protseq_endpoint_count: 0,
                _pad0: 0,
                rpc_protseq_endpoint: std::ptr::null(),
                reserved: 0,
                interpreter_info: std::ptr::null(),
                // `RPC_CLIENT_INTERFACE::Flags` must stay 0 here: the vendor
                // ifspec carries 0x06000000, which makes rpcrt4 take the
                // NDR64-capable dispatch path. That path needs the NDR64
                // format tables the vendor DLL keeps in its own.rdata;
                // with only the NDR32 (v2.0) interpreter tables this client
                // carries, the call dies with RPC_S_INTERNAL_ERROR (0x6e6)
                // inside the type-format interpreter. Flags = 0 selects the
                // single NDR v2.0 syntax the service actually offers, and the
                // marshalled wire bytes are unchanged (the negotiated
                // transfer syntax is still NDR32 v2.0).
                flags: 0,
                _pad1: 0,
            },
        });
        let t: &mut ProxyTables = &mut tables;
        t.stub_desc.rpc_interface_information = &raw const t.if_spec;
        // `pHandle` = pointer to the implicit handle variable; single
        // indirection.
        t.stub_desc.p_handle = &raw const t.handle;
        t.stub_desc.p_format_types = t.ndr.type_format.as_ptr();
        t.proxy_info.p_stub_desc = &raw const t.stub_desc;
        t.proxy_info.proc_format_string = t.ndr.proc_format.as_ptr();
        t.proxy_info.format_string_offset = t.ndr.proc_offsets.as_ptr();
        // A standalone NDR32 identifier, distinct from
        // `MIDL_SYNTAX_INFO::TransferSyntax` (standard client convention).
        t.proxy_info.p_transfer_syntax = &raw const t.transfer_syntax;
        t.proxy_info.p_syntax_info = &raw const t.syntax_info;
        t.stub_desc.proxy_server_info = &raw const t.proxy_info as usize;
        t.syntax_info.proc_string = t.ndr.proc_format.as_ptr();
        t.syntax_info.fmt_string_offset = t.ndr.proc_offsets.as_ptr();
        t.syntax_info.type_string = t.ndr.type_format.as_ptr();
        t.if_spec.interpreter_info = &raw const t.proxy_info as *const c_void;
        tables
    }
}

// ---------------------------------------------------------------------------
// Minimal JSON writer/reader for the elevoc_rpc_protocol envelope. Requests
// are built by us with exact field order; responses only need scalar/array
// extraction from the `"Content"` object.
// ---------------------------------------------------------------------------

fn json_escape_into(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

/// Ordered JSON object builder matching the vendor envelope shape.
#[derive(Default)]
struct Envelope {
    fields: Vec<(&'static str, String, bool)>,
}

impl Envelope {
    fn new(request: &'static str) -> Self {
        let mut env = Envelope::default();
        env.string("request", request);
        env
    }

    fn string(&mut self, key: &'static str, value: &str) -> &mut Self {
        let mut escaped = String::with_capacity(value.len() + 2);
        escaped.push('"');
        json_escape_into(&mut escaped, value);
        escaped.push('"');
        self.fields.push((key, escaped, true));
        self
    }

    fn number(&mut self, key: &'static str, value: i64) -> &mut Self {
        self.fields.push((key, value.to_string(), false));
        self
    }

    fn opt_endpoint(&mut self, endpoint: &str) -> &mut Self {
        if !endpoint.is_empty() {
            self.string("endpoint", endpoint);
        }
        self
    }

    /// Append `elevoc_rpc_protocol` + `token` exactly where the vendor writer
    /// places them.
    fn seal(&mut self, token: &str) -> String {
        self.string("elevoc_rpc_protocol", "RPC/2.2");
        self.string("token", token);
        self.finish()
    }

    fn finish(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push('{');
        for (i, (k, v, _)) in self.fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('"');
            out.push_str(k);
            out.push_str("\":");
            out.push_str(v);
        }
        out.push('}');
        out
    }
}

/// Extract the `"Content"` object body from a response envelope.
fn content_object(response: &str) -> Option<&str> {
    let key = response.find("\"Content\"")?;
    let rest = &response[key + 9..];
    let open = rest.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for (i, ch) in rest[open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[open..=open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read a scalar JSON value (`string`, number, bool) for `key` inside `obj`.
fn json_value<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    let idx = obj.find(&pat)?;
    let after = idx + pat.len();
    let rest = &obj[after..];
    let colon = rest.find(':')?;
    let val = rest[colon + 1..].trim_start();
    if val.is_empty() {
        return None;
    }
    Some(match val.as_bytes()[0] {
        b'"' => {
            let end = val[1..].find('"').map(|e| e + 1)?;
            &val[1..end]
        }
        b'{' | b'[' | b't' | b'f' | b'n' | b'-' | b'0'..=b'9' => val,
        _ => return None,
    })
}

fn json_string<'a>(obj: &'a str, key: &str) -> Option<&'a str> {
    let v = json_value(obj, key)?;
    if v.starts_with('"') || v.starts_with('{') || v.starts_with('[') {
        return None;
    }
    Some(v.trim_end_matches([',', ' ', '\t', '\r', '\n']))
}

fn json_bool(obj: &str, key: &str) -> Option<bool> {
    let v = json_value(obj, key)?;
    if v.starts_with("true") {
        return Some(true);
    }
    if v.starts_with("false") {
        return Some(false);
    }
    let n: i64 = v
        .split([',', ' ', '\t', '\r', '\n', '}', ']'])
        .next()?
        .parse()
        .ok()?;
    Some(n != 0)
}

fn json_int(obj: &str, key: &str) -> Option<i64> {
    let v = json_value(obj, key)?;
    v.split([',', ' ', '\t', '\r', '\n', '}', ']'])
        .next()?
        .parse()
        .ok()
}

/// Collect `"key": [ "a", "b",... ]` string arrays (e.g. `tokens`,
/// `supported_functions`).
fn json_string_array(obj: &str, key: &str) -> Vec<String> {
    let Some(v) = json_value(obj, key) else {
        return Vec::new();
    };
    if !v.starts_with('[') {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut rest = &v[1..];
    while let Some(q) = rest.find('"') {
        let after = &rest[q + 1..];
        let Some(end) = after.find('"') else { break };
        out.push(after[..end].to_string());
        rest = &after[end + 1..];
        if let Some(c) = rest.find([',', ']']) {
            if rest.as_bytes()[c] == b']' {
                break;
            }
            rest = &rest[c + 1..];
        } else {
            break;
        }
    }
    out
}

/// Per-request nonce — the vendor fills `token` with a randomized short
/// string (`rand()`→obfuscated itoa); only presence is
/// server-visible, so a fresh pseudo-random value is equivalent.
fn make_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 20))
        .unwrap_or(0x9e37_79b9);
    let stack = std::ptr::addr_of!(nanos) as u64;
    let mut x = nanos ^ stack.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut s = String::with_capacity(8);
    for _ in 0..8 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.push(char::from_digit((x % 36) as u32, 36).unwrap_or('0'));
    }
    s
}

// ---------------------------------------------------------------------------
// COM: IMMDeviceEnumerator → default capture endpoint ID (the `"endpoint"`
// request field). The vendor resolves it via CoInitializeEx + CoCreateInstance
// + GetDefaultAudioEndpoint(eCapture, eConsole) + IMMDevice::GetId.
// ---------------------------------------------------------------------------

const CLSID_MMDEVICE_ENUMERATOR: [u8; 16] = [
    0x95, 0x03, 0xde, 0xbc, 0x2f, 0xe5, 0x7c, 0x46, 0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e,
];
const IID_IMMDEVICE_ENUMERATOR: [u8; 16] = [
    0xd2, 0x64, 0x56, 0xa9, 0x14, 0x96, 0x35, 0x4f, 0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6,
];
const COINIT_MULTITHREADED: u32 = 0;
const CLSCTX_ALL: u32 = 0x17;
const E_CAPTURE: i32 = 1;
const E_CONSOLE: i32 = 0;

#[repr(C)]
struct ComObject {
    vtbl: *const *const c_void,
}

unsafe fn com_release(obj: *mut ComObject) {
    unsafe {
        let release: unsafe extern "C" fn(*mut ComObject) -> u32 =
            std::mem::transmute(*(*obj).vtbl.add(2));
        release(obj);
    }
}

/// Default capture-endpoint ID string (`{0.0.1.…}.{guid}`), or `None` when no
/// capture device exists / COM fails — the vendor then omits `"endpoint"`.
fn default_capture_endpoint() -> Option<String> {
    unsafe {
        // CoInitializeEx(NULL, COINIT_MULTITHREADED); S_FALSE is fine.
        let _ = CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED);
        struct ComGuard;
        impl Drop for ComGuard {
            fn drop(&mut self) {
                unsafe { CoUninitialize() };
            }
        }
        let _guard = ComGuard;

        let mut enumerator: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_MMDEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut enumerator,
        );
        if hr < 0 || enumerator.is_null() {
            return None;
        }
        let enumerator = enumerator.cast::<ComObject>();

        // IMMDeviceEnumerator::GetDefaultAudioEndpoint(dataFlow, role, &dev)
        let mut device: *mut c_void = std::ptr::null_mut();
        let get_default: unsafe extern "C" fn(*mut ComObject, i32, i32, *mut *mut c_void) -> i32 =
            std::mem::transmute(*(*enumerator).vtbl.add(4));
        let hr = get_default(enumerator, E_CAPTURE, E_CONSOLE, &mut device);
        com_release(enumerator);
        if hr < 0 || device.is_null() {
            return None;
        }
        let device = device.cast::<ComObject>();

        // IMMDevice::GetId(&lpwstr)
        let mut id_ptr: *mut u16 = std::ptr::null_mut();
        let get_id: unsafe extern "C" fn(*mut ComObject, *mut *mut u16) -> i32 =
            std::mem::transmute(*(*device).vtbl.add(5));
        let hr = get_id(device, &mut id_ptr);
        com_release(device);
        if hr < 0 || id_ptr.is_null() {
            return None;
        }
        let mut len = 0usize;
        while *id_ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(id_ptr, len));
        CoTaskMemFree(id_ptr.cast());
        Some(text)
    }
}

// ---------------------------------------------------------------------------
// NativeElevocRpc — pure-Rust ncalrpc client
// ---------------------------------------------------------------------------

/// Native transport speaking `elevoc_rpc_protocol` to ElevocControlService
/// over `ncalrpc:[ElevocRpcEndpoint]` through the system `rpcrt4` NDR engine.
/// `load()` reproduces the vendor `Initialize`→`Login` sequence exactly:
/// bind → init warm-ups → `GetAuthorizedTokens(user="")` →
/// `GetConfigFileContent` → mic-state warm-ups → internal endpoint.
pub struct NativeElevocRpc {
    tables: Box<ProxyTables>,
}

/// Own the complete [in,out,string,size_is] storage across one round trip.
/// Keeping the invocation behind this boundary also allows regression tests
/// to exercise long replies without asking a live service to fabricate one.
fn exchange_json(
    request_json: &str,
    invoke: impl FnOnce(&mut [u16; RPC_BUFFER_CHARS]) -> Result<()>,
) -> Result<String> {
    let mut buf = [0u16; RPC_BUFFER_CHARS];
    for (index, unit) in request_json.encode_utf16().enumerate() {
        if index >= RPC_REQUEST_CHARS - 1 {
            return Err(LctrlError::InvalidArgument {
                detail: format!("Elevoc request exceeds {} wchars", RPC_REQUEST_CHARS - 1),
            });
        }
        buf[index] = unit;
    }
    invoke(&mut buf)?;
    let len = buf
        .iter()
        .position(|c| *c == 0)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc RPC returned an unterminated response".to_string(),
        })?;
    if len == 0 {
        return Err(LctrlError::ChannelUnavailable {
            channel: "Elevoc RPC returned an empty response".to_string(),
        });
    }
    String::from_utf16(&buf[..len]).map_err(|_| LctrlError::ChannelUnavailable {
        channel: "Elevoc RPC returned non-UTF16 response".to_string(),
    })
}

impl NativeElevocRpc {
    /// Bind `ncalrpc:[ElevocRpcEndpoint]` and run the login handshake the
    /// vendor SDK performs before any query works (every request handler
    /// returns -1 "Please Init first" until the binding exists and the
    /// `GetAuthorizedTokens` exchange has run).
    pub fn load() -> Result<Self> {
        let mut tables = ProxyTables::new();
        bind(&mut tables.handle)?;
        let rpc = Self { tables };
        rpc.login()?;
        Ok(rpc)
    }

    /// `Login`: authorized-token exchange plus the SDK's warm-up
    /// queries in their original order. Warm-up failures are non-fatal, as in
    /// the SDK; only the authorization exchange gates login success.
    fn login(&self) -> Result<()> {
        // Init warm-ups (client->Init): downlink mode probe,
        // endpoint ability probe, headset status.
        let endpoint = default_capture_endpoint().unwrap_or_default();
        let _ = self.rpc_get_mode("app_downlink", "");
        let _ = self.rpc_get_functions_by_endpoint("app_downlink");
        let _ = self.rpc_simple("GetHeadSetStatus");

        // Connect(appkey) (client->Connect → RpcGetTokens
        //): skipping this exchange caused later queries to
        // fault on the observed service version. Empty or absent `user`
        // worked on that target; a non-whitelisted value faulted the handler.
        // Accept either `auth_status` or a non-empty `tokens` list.
        let mut env = Envelope::new("GetAuthorizedTokens");
        env.string("elevoc_rpc_protocol", "RPC/2.2");
        env.string("token", &make_token());
        env.string("user", "");
        let body = env.finish();
        let resp = self.request(&body)?;
        let content = content_object(&resp).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc GetAuthorizedTokens returned no Content".to_string(),
        })?;
        let authorized = json_bool(content, "auth_status") == Some(true)
            || !json_string_array(content, "tokens").is_empty();
        if !authorized {
            return Err(LctrlError::ChannelUnavailable {
                channel: "Elevoc GetAuthorizedTokens authorization exchange failed".to_string(),
            });
        }

        // Query(out) (client->Query → RpcGetConfigData
        //): request=GetConfigFileContent, buffer_len=0x1000.
        let mut env = Envelope::new("GetConfigFileContent");
        env.number("buffer_len", 0x1000);
        let body = env.seal(&make_token());
        let _ = self.request(&body);

        // GetMicState(out3) warm-ups (export): supported
        // functions, voice-id state, speaker_extraction ability.
        let _ = self.rpc_get_functions();
        let _ = self.rpc_is_voiceid_setted(&endpoint);
        let _ = self.rpc_get_functions_by_endpoint("speaker_extraction");

        Ok(())
    }

    /// One opnum-0 round trip: UTF-16 JSON out, response JSON back in the
    /// same buffer. Mirrors `SafeRpcRequest`.
    fn request(&self, request_json: &str) -> Result<String> {
        exchange_json(request_json, |buf| rpc_call_guarded(&self.tables, buf))
    }

    fn rpc_simple(&self, request: &'static str) -> Result<String> {
        let mut env = Envelope::new(request);
        let body = env.seal(&make_token());
        self.request(&body)
    }

    /// `RpcGetMode`: `{request,link,endpoint?,proto,token}` →
    /// `Content.mode` name → int, then the `algType`-contains-`token` probe
    /// that upgrades 1→6 (`shared_td`) and 2→7 (`private_td`).
    fn rpc_get_mode(&self, link: &str, endpoint: &str) -> Result<i32> {
        let mut env = Envelope::new("GetMode");
        env.string("link", link);
        env.opt_endpoint(endpoint);
        let body = env.seal(&make_token());
        let resp = self.request(&body)?;
        let content = content_object(&resp).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc GetMode returned no Content".to_string(),
        })?;
        let mode_name = json_string(content, "mode").unwrap_or("environmental");
        let mut mode = MODE_NAMES
            .iter()
            .position(|n| *n == mode_name)
            .map(|i| i as i32)
            .unwrap_or(0);
        if matches!(mode, 1 | 2) {
            // The SDK upgrades shared/private to the legacy time-domain
            // aliases when the endpoint's algType does NOT start with "AI"
            // (`com = algType.find("AI")`, `com != 0` → td; at
            // — matches SetAlgType sending
            // "Non-AI" for td modes).
            if let Some(alg) = self.rpc_get_alg_type(endpoint) {
                if !alg.starts_with("AI") {
                    mode += 5;
                }
            }
        }
        Ok(mode)
    }

    fn rpc_get_alg_type(&self, endpoint: &str) -> Option<String> {
        let mut env = Envelope::new("GetAlgType");
        env.opt_endpoint(endpoint);
        let body = env.seal(&make_token());
        let resp = self.request(&body).ok()?;
        let content = content_object(&resp)?;
        json_string(content, "algroithm_type").map(str::to_string)
    }

    /// `RpcSetAlgType`: `{request:"SetAlgType",algroithm_type,
    /// endpoint?,proto,token}` — sent by `SetEffectMode` before every mode
    /// dispatch ("Non-AI" for td-alias modes, "AI" otherwise); the vendor
    /// ignores its result.
    fn rpc_set_alg_type(&self, alg_type: &str, endpoint: &str) -> Result<()> {
        let mut env = Envelope::new("SetAlgType");
        env.string("algroithm_type", alg_type);
        env.opt_endpoint(endpoint);
        let body = env.seal(&make_token());
        self.request(&body).map(|_| ())
    }

    /// `RpcSetMode`: `{request:"SetMode",link,mode,CALLER:"SDK",
    /// proto,token,endpoint?}`. Returns 1 on send success.
    fn rpc_set_mode(&self, link: &str, mode_name: &str, endpoint: &str) -> Result<i32> {
        let mut env = Envelope::new("SetMode");
        env.string("link", link);
        env.string("mode", mode_name);
        env.string("CALLER", "SDK");
        env.seal(&make_token());
        env.opt_endpoint(endpoint);
        let body = env.finish();
        self.request(&body).map(|_| 1)
    }

    /// `RpcIsVoiceidSetted`: `{request,endpoint,proto,token}` →
    /// `Content.is_voiceid_setted` → 2 enrolled / 0 unset.
    fn rpc_is_voiceid_setted(&self, endpoint: &str) -> Result<i32> {
        let mut env = Envelope::new("IsVoiceidSetted");
        if !endpoint.is_empty() {
            env.string("endpoint", endpoint);
        }
        let body = env.seal(&make_token());
        let resp = self.request(&body)?;
        let content = content_object(&resp).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc IsVoiceidSetted returned no Content".to_string(),
        })?;
        Ok(
            if json_bool(content, "is_voiceid_setted") == Some(true)
                || json_int(content, "is_voiceid_setted") == Some(2)
            {
                2
            } else {
                0
            },
        )
    }

    /// `RpcGetSupportedFunctionsByEndpoint`:
    /// `{request:"GetFunctionsByEndpoint",endpoint,proto,token}` →
    /// `Content.supported_functions` string list.
    fn rpc_get_functions_by_endpoint(&self, endpoint: &str) -> Result<Vec<String>> {
        let mut env = Envelope::new("GetFunctionsByEndpoint");
        env.opt_endpoint(endpoint);
        let body = env.seal(&make_token());
        let resp = self.request(&body)?;
        let content = content_object(&resp).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc GetFunctionsByEndpoint returned no Content".to_string(),
        })?;
        Ok(json_string_array(content, "supported_functions"))
    }

    /// `GetFunctions` warm-up (`GetFunctions` request via client->Query).
    fn rpc_get_functions(&self) -> Result<Vec<String>> {
        let mut env = Envelope::new("GetFunctions");
        let body = env.seal(&make_token());
        let resp = self.request(&body)?;
        let content = content_object(&resp).ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "Elevoc GetFunctions returned no Content".to_string(),
        })?;
        Ok(json_string_array(content, "supported_functions"))
    }
}

impl Drop for NativeElevocRpc {
    fn drop(&mut self) {
        unsafe {
            let mut handle = self.tables.handle;
            RpcBindingFree(&mut handle);
        }
    }
}

impl ElevocRpc for NativeElevocRpc {
    /// `GetCurrentMode(NULL)`: resolve the default capture endpoint, send
    /// `GetMode{link:"app_uplink"}` — returns -1 on any failure, exactly as
    /// the vendor export does (init flag off / no default endpoint / RPC
    /// fault).
    fn current_mode(&self) -> Result<i32> {
        let endpoint = default_capture_endpoint().unwrap_or_default();
        self.rpc_get_mode("app_uplink", &endpoint).or(Ok(-1))
    }

    /// `SetEffectMode(mode, NULL)`: voice-id gate (4), legacy alias remap
    /// (6→1, 7→2), endpoint ability gate (2), then `SetMode`. Result codes
    /// map identically to the vendor export.
    fn set_effect_mode(&self, mode: u32) -> Result<()> {
        if mode == 4 {
            match self.voice_id_status() {
                Ok(2) => {}
                Ok(_) => {
                    return Err(LctrlError::InvalidArgument {
                        detail: "voiceid mode requires an enrolled VoiceID profile".to_string(),
                    });
                }
                Err(_) => {
                    return Err(LctrlError::ChannelUnavailable {
                        channel: "Elevoc SetEffectMode RPC or default-endpoint failure".to_string(),
                    });
                }
            }
        }
        let remapped = match mode {
            6 => 1,
            7 => 2,
            m => m,
        };
        if remapped >= MODE_NAMES.len() as u32 {
            // SDK returns 3 ("not trans") for out-of-domain requests; mapped
            // to Unsupported as a request-side rejection.
            return Err(LctrlError::Unsupported {
                feature: format!("noise-cancel mode {mode} is outside the Elevoc domain"),
            });
        }
        let endpoint = default_capture_endpoint().unwrap_or_default();
        if endpoint.is_empty() {
            return Err(LctrlError::ChannelUnavailable {
                channel: "Elevoc SetEffectMode RPC or default-endpoint failure".to_string(),
            });
        }
        let mode_name = MODE_NAMES[remapped as usize];
        // The endpoint's capability list does not enumerate the mode names,
        // so it cannot gate the request; the service answers with result 2
        // when the mode is unsupported, which the caller maps to Unsupported.
        // `SetEffectMode` then announces the algorithm family: Non-AI for
        // the td-alias inputs / → SetAlgType), AI for
        // everything else. The result is ignored by the vendor.
        let alg_type = if matches!(mode, 6 | 7) {
            "Non-AI"
        } else {
            "AI"
        };
        let _ = self.rpc_set_alg_type(alg_type, &endpoint);
        self.rpc_set_mode("uplink", mode_name, &endpoint)
            .map(|_| ())
    }

    /// `QueryVoiceIDStatus()`: `IsVoiceidSetted{endpoint}` → 0/2; -1 on
    /// failure (the SDK's headset-side query path collapses to the same
    /// envelope with the default endpoint).
    fn voice_id_status(&self) -> Result<i32> {
        let endpoint = default_capture_endpoint().unwrap_or_default();
        self.rpc_is_voiceid_setted(&endpoint).or(Ok(-1))
    }
}

fn bind(handle_slot: *mut RpcHandle) -> Result<()> {
    unsafe {
        let mut string_binding: *mut u16 = std::ptr::null_mut();
        let status = RpcStringBindingComposeW(
            std::ptr::null(),
            PROTO_SEQ.as_ptr(),
            std::ptr::null(),
            ENDPOINT_NAME.as_ptr(),
            std::ptr::null(),
            &mut string_binding,
        );
        if status != 0 || string_binding.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("Elevoc ncalrpc binding compose failed (status {status})"),
            });
        }
        let mut handle = std::ptr::null_mut();
        let status = RpcBindingFromStringBindingW(string_binding, &mut handle);
        RpcStringFreeW(&mut string_binding);
        if status != 0 || handle.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "Elevoc ncalrpc:[ElevocRpcEndpoint] bind failed (status {status}) — ElevocService must be running"
                ),
            });
        }
        std::ptr::write(handle_slot, handle);
        Ok(())
    }
}

/// Speaker-side (downlink) observation from the SmartEngine F4006 cache.
/// The registry is a write-only mirror maintained by the Vantage plugin;
/// no service consumes it, so this is display state, not control state.
fn read_f4006_speaker_mirror() -> Option<bool> {
    use std::os::windows::ffi::OsStrExt;
    const KEY: &str = r"SOFTWARE\Lenovo\SmartEngine\modules\F4006";
    const VALUE: &str = "noise_speaker_mode";
    let key_wide: Vec<u16> = std::ffi::OsStr::new(KEY)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value_wide: Vec<u16> = std::ffi::OsStr::new(VALUE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut buffer = [0u16; 8];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        windows_sys::Win32::System::Registry::RegGetValueW(
            windows_sys::Win32::System::Registry::HKEY_CURRENT_USER,
            key_wide.as_ptr(),
            value_wide.as_ptr(),
            windows_sys::Win32::System::Registry::RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..len]);
    match text.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests — these run under `cargo check --target x86_64-pc-windows-msvc
// --all-targets` on Linux (the crate never links there), so they are
// compile-checked, not executed, unless run on Windows.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_exchange_accepts_replies_beyond_the_request_limit() {
        for units in [RPC_REQUEST_CHARS, RPC_BUFFER_CHARS - 1] {
            let expected = "x".repeat(units);
            let result = exchange_json(r#"{"request":"GetConfigFileContent"}"#, |buf| {
                // A valid reply can fill the advertised wire capacity even
                // when the request itself fits the smaller admission limit.
                buf[..units].fill(u16::from(b'x'));
                buf[units] = 0;
                Ok(())
            })
            .expect("the complete conforming reply fits");
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn json_exchange_rejects_oversized_request_before_invoking_rpc() {
        let error = exchange_json(&"x".repeat(RPC_REQUEST_CHARS), |_| {
            panic!("oversized request reached RPC")
        })
        .expect_err("reserve one UTF-16 unit for the request terminator");
        assert!(matches!(error, LctrlError::InvalidArgument { .. }));
    }

    #[test]
    fn json_exchange_rejects_unterminated_reply() {
        let error = exchange_json("{}", |buf| {
            buf.fill(u16::from(b'x'));
            Ok(())
        })
        .expect_err("missing terminator must not be accepted as a full response");
        assert!(matches!(error, LctrlError::ChannelUnavailable { .. }));
    }

    #[test]
    fn json_exchange_propagates_rpc_failure_without_reading_request_as_reply() {
        let error = exchange_json("{}", |_| {
            Err(LctrlError::ChannelUnavailable {
                channel: "RPC transport failed".to_string(),
            })
        })
        .expect_err("request contents are not a successful response");
        assert!(
            matches!(error, LctrlError::ChannelUnavailable { channel } if channel == "RPC transport failed")
        );
    }

    #[test]
    fn envelope_writes_fields_in_vendor_order() {
        let mut env = Envelope::new("GetMode");
        env.string("link", "app_uplink");
        env.seal("7");
        env.opt_endpoint("");
        assert_eq!(
            env.finish(),
            r#"{"request":"GetMode","link":"app_uplink","elevoc_rpc_protocol":"RPC/2.2","token":"7"}"#
        );
    }

    #[test]
    fn envelope_set_mode_vendor_order() {
        let mut env = Envelope::new("SetMode");
        env.string("link", "uplink");
        env.string("mode", "private");
        env.string("CALLER", "SDK");
        env.seal("3");
        env.opt_endpoint("mic01");
        assert_eq!(
            env.finish(),
            r#"{"request":"SetMode","link":"uplink","mode":"private","CALLER":"SDK","elevoc_rpc_protocol":"RPC/2.2","token":"3","endpoint":"mic01"}"#
        );
    }

    #[test]
    fn envelope_escapes_json_specials() {
        let mut env = Envelope::new("IsVoiceidSetted");
        env.string("endpoint", "a\\b\"c\nd");
        env.seal("1");
        assert_eq!(
            env.finish(),
            r#"{"request":"IsVoiceidSetted","endpoint":"a\\b\"c\nd","elevoc_rpc_protocol":"RPC/2.2","token":"1"}"#
        );
    }

    #[test]
    fn envelope_omits_empty_endpoint() {
        let mut env = Envelope::new("SetMode");
        env.string("link", "uplink");
        env.string("mode", "shared");
        env.string("CALLER", "SDK");
        env.seal("x");
        env.opt_endpoint("");
        assert!(!env.finish().contains("endpoint"));
    }

    #[test]
    fn envelope_conformance_response_extraction() {
        let resp = r#"{"Header":{"status":0,"protocol":"RPC/2.2"},"Content":{"mode":"private","supported_functions":["environmental","private"]}}"#;
        let content = content_object(resp).expect("content");
        assert_eq!(json_string(content, "mode"), Some("private"));
        let supported = json_string_array(content, "supported_functions");
        assert_eq!(supported, vec!["environmental", "private"]);
    }

    #[test]
    fn json_value_reads_string_number_bool() {
        let obj = r#"{"a":"x","b":4,"c":true,"d":"}"}"#;
        assert_eq!(json_string(obj, "a"), Some("x"));
        assert_eq!(json_int(obj, "b"), Some(4));
        assert_eq!(json_bool(obj, "c"), Some(true));
        // string containing '}' must not truncate early
        assert_eq!(json_string(obj, "d"), Some("}"));
    }

    #[test]
    fn voice_id_status_maps_server_results() {
        // Server returns Content.is_voiceid_setted bool; SDK maps to 0/2.
        let set = r#"{"Content":{"is_voiceid_setted":true}}"#;
        let unset = r#"{"Content":{"is_voiceid_setted":false}}"#;
        assert_eq!(
            json_bool(content_object(set).unwrap(), "is_voiceid_setted"),
            Some(true)
        );
        assert_eq!(
            json_bool(content_object(unset).unwrap(), "is_voiceid_setted"),
            Some(false)
        );
    }

    #[test]
    fn ndr_struct_layouts_match_c_abi() {
        use std::mem::{align_of, size_of};
        // RPC_SYNTAX_IDENTIFIER is GUID(16)+RPC_VERSION(4), C alignment 4.
        assert_eq!(size_of::<RpcSyntaxIdentifier>(), 20);
        assert_eq!(align_of::<RpcSyntaxIdentifier>(), 4);
        // MIDL_STUB_DESC is pointer-dense; size sanity only.
        assert!(size_of::<MidlStubDesc>() >= 0x60);
        assert_eq!(size_of::<MidlSyntaxInfo>(), 0x50);
    }

    #[test]
    fn if_spec_fields_match_vendor_interface() {
        let t = ProxyTables::new();
        assert_eq!(t.if_spec.length as usize, size_of::<RpcClientInterface>());
        assert_eq!(t.if_spec.interface_id.syntax_guid, ELEVOC_IF_GUID);
        assert_eq!(t.if_spec.interface_id.syntax_version.major_version, 4);
        assert_eq!(t.if_spec.interface_id.syntax_version.minor_version, 0);
        // `Flags` must stay 0: the vendor value 0x06000000 drives rpcrt4
        // onto the NDR64-capable path, which needs NDR64 format tables this
        // client does not carry (the call then fails with 0x6e6).
        assert_eq!(t.if_spec.flags, 0);
        assert_eq!(t.syntax_info.transfer_syntax.syntax_guid, NDR32_SYNTAX_GUID);
        assert_eq!(
            t.syntax_info.transfer_syntax.syntax_version.major_version,
            2
        );
        // The vendor advertises both transfer syntaxes (NDR32 + NDR64).
        assert_eq!(t.proxy_info.n_count, 2);
        assert_eq!(
            t.syntax_info2.transfer_syntax.syntax_guid,
            NDR64_SYNTAX_GUID
        );
        assert_eq!(
            t.proxy_info.p_syntax_info as usize,
            &t.syntax_info as *const _ as usize
        );
        // The two MIDL_SYNTAX_INFO records must be adjacent: NdrClientCall3
        // indexes `pSyntaxInfo` as an array of `nCount` records.
        assert_eq!(
            &t.syntax_info2 as *const _ as usize - &t.syntax_info as *const _ as usize,
            std::mem::size_of::<MidlSyntaxInfo>()
        );
        // Interior pointers point back into the same allocation.
        assert_eq!(
            t.proxy_info.p_stub_desc as usize,
            &t.stub_desc as *const _ as usize
        );
        assert_eq!(
            t.if_spec.interpreter_info as usize,
            &t.proxy_info as *const _ as usize
        );
    }
}

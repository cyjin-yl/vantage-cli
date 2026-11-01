//! Dolby DAX RPC control through a pure-Rust `rpcrt4.dll` NDR client.
//!
//! Contract recovered by static analysis of TB14P-EZRA vendor binaries plus a
//! live read-only probe; see `docs/RE-interfaces.md` §12. `DAX3API.exe`
//! (`DolbyDAXAPI` service, SYSTEM) registers interface
//! `11c6212d-d5f3-4500-ab16-634dc63037be` v1.0 / NDR v2.0 on the ALPC
//! endpoint `ncalrpc:[DaxRpcEndpoint]` with `RPC_IF_AUTOLISTEN |
//! RPC_IF_ALLOW_LOCAL_ONLY`. This module reproduces the vendor
//! `DolbyRpcClient.dll` call sequence on the same endpoint without loading
//! any Lenovo-delivered binary: `rpcrt4.dll` (the Windows RPC runtime) does
//! marshaling, driven by MIDL interpreter metadata generated in-tree
//! from the recovered interface signatures (`ndr` module — the `-Oicf`
//! encoding is canonical, so the emitted descriptors coincide with the
//! proc strings the vendor stub carried).
//!
//! Call sequence replicated from the vendor DLL (`Initialize`):
//!
//! 1. `RpcStringBindingComposeW("ncalrpc", NULL, "DaxRpcEndpoint")` —
//!    no object UUID on the binding (vendor-verified; the UUID is carried in
//!    the opnum-4 handshake instead).
//! 2. `RpcBindingFromStringBindingW` → session binding handle (lazy bind).
//! 3. `NdrClientCall3(proc 0)`: `[in] handle_t` binding → `[out]` session
//!    context token. All later procedures take that token as parameter 0;
//!    the runtime routes the call over the binding stored inside it.
//! 4. `NdrClientCall3(proc 4)`: `[in]` object-UUID BSTR → `[out]` u16; the
//!    vendor ignores the result, so a failure is reported but not fatal.
//!    BSTR parameters marshal through `FC_USER_MARSHAL` quad index 0, which
//!    this module binds to oleaut32's `BSTR_User*` routines — identical to
//!    the vendor client's quad table.
//!
//! | procedure | opnum | args (after context token) | semantics |
//! |-----------|-------|----------------------------|-----------|
//! | GetDolbyEnabled        | 5  | `[out] u16`           | on/off read (0/1) |
//! | SetDolbyEnabled        | 6  | `[in] u8`             | on/off write |
//! | GetActiveProfile       | 7  | `[out] u32`           | preset read [0,5] |
//! | SetActiveProfile       | 8  | `[in] u32`            | preset write |
//! | IsSpatialAudioSupported| 2  | 4 × `[out] u32`       | `p0 > 3 ‖ p1 ≥ 0x7853` |
//! | GetSKU                 | 37 | `[out] BSTR*`         | SKU-name copy success |
//!
//! The licensed APO DSP inside DAX3API is never touched; only the
//! toggle/profile opnums are exercised.
//!
//! The interface descriptor advertises a single transfer syntax (NDR v2.0).
//! The vendor descriptor also declares NDR64, but with its multi-syntax
//! bits set rpcrt4 negotiates NDR64 and then needs the vendor's NDR64
//! format tables, which this client does not carry (`RPC_S_INTERNAL_ERROR`
//! otherwise). Keeping one syntax pins the negotiated transfer syntax to
//! NDR v2.0 — the same bytes the vendor client emits.
//!
//! `NdrClientCall3` reports transport failures by raising SEH exceptions
//! (the vendor client wraps its calls in `__try/__except`). Stable Rust has
//! no SEH, so calls run inside a vectored-handler + `RtlCaptureContext`/
//! `RtlRestoreContext` guard that converts RPC status exceptions into
//! `ChannelUnavailable` errors instead of aborting the process.

use std::time::Duration;

use lctrl_core::{ApplyMode, ChangeReport, DolbyProfile, DolbyState, LctrlError, Result};
use lctrl_hal::AudioControl;
use lctrl_hal::poll_readback;

#[path = "ndr.rs"]
mod ndr;

/// Sealed seam over the DAX RPC session so [`WindowsDolbyAudio`] stays
/// testable without a running DAX3API.
pub trait DolbyRpc: Send + Sync {
    fn dolby_enabled(&self) -> Result<bool>;
    fn set_dolby_enabled(&self, enabled: bool) -> Result<()>;
    fn active_profile(&self) -> Result<u32>;
    fn set_active_profile(&self, index: u32) -> Result<()>;
    fn sku(&self) -> Result<u32>;
    fn spatial_supported(&self) -> Result<bool>;
}

/// `AudioControl` backed by a [`DolbyRpc`] transport.
pub struct WindowsDolbyAudio<R: DolbyRpc> {
    rpc: R,
}

impl<R: DolbyRpc> WindowsDolbyAudio<R> {
    #[must_use]
    pub const fn new(rpc: R) -> Self {
        Self { rpc }
    }

    #[must_use]
    pub const fn rpc(&self) -> &R {
        &self.rpc
    }

    fn read_state(&self) -> Result<DolbyState> {
        let enabled = self.rpc.dolby_enabled()?;
        let raw_profile = self.rpc.active_profile()?;
        let active_profile = DolbyProfile::from_raw(raw_profile as u8).ok_or_else(|| {
            LctrlError::ChannelUnavailable {
                channel: format!(
                    "DAX RPC returned profile index {raw_profile}; valid range is [0,5]"
                ),
            }
        })?;
        let sku = self.rpc.sku().ok();
        let spatial_supported = self.rpc.spatial_supported().ok();
        Ok(DolbyState {
            enabled,
            active_profile,
            sku,
            spatial_supported,
        })
    }
}

impl<R: DolbyRpc> AudioControl for WindowsDolbyAudio<R> {
    fn dolby_state(&self) -> Result<DolbyState> {
        self.read_state()
    }

    fn set_dolby_profile(
        &self,
        profile: DolbyProfile,
        apply: ApplyMode,
    ) -> Result<ChangeReport<DolbyState>> {
        let previous = self.read_state()?;
        let mut requested = previous;
        requested.enabled = profile != DolbyProfile::Off;
        requested.active_profile = profile;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        if profile == DolbyProfile::Off {
            self.rpc.set_dolby_enabled(false)?;
        } else {
            self.rpc.set_dolby_enabled(true)?;
            self.rpc.set_active_profile(u32::from(profile.raw()))?;
        }
        let actual = poll_readback(&requested, 10, Duration::from_millis(50), || {
            self.read_state()
        })?;
        Ok(ChangeReport::committed(previous, requested, actual))
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust ncalrpc transport.
// ---------------------------------------------------------------------------

mod rpc {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::mem::MaybeUninit;
    use std::ptr;
    use std::sync::LazyLock;

    use lctrl_core::{LctrlError, Result};
    use windows_sys::Win32::Foundation::{SysAllocString, SysFreeString};
    use windows_sys::Win32::System::Com::Marshal::{
        BSTR_UserFree, BSTR_UserMarshal, BSTR_UserSize, BSTR_UserUnmarshal,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, CONTEXT, EXCEPTION_POINTERS, RtlCaptureContext,
        RtlRestoreContext,
    };
    use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HeapFree};
    use windows_sys::Win32::System::Rpc::{
        MIDL_SERVER_INFO, MIDL_STUB_DESC, MIDL_STUB_DESC_0, MIDL_STUBLESS_PROXY_INFO,
        MIDL_SYNTAX_INFO, NdrClientCall3, RPC_CLIENT_INTERFACE, RPC_SYNTAX_IDENTIFIER, RPC_VERSION,
        RpcBindingFree, RpcBindingFromStringBindingW, RpcStringBindingComposeW, RpcStringFreeW,
        USER_MARSHAL_ROUTINE_QUADRUPLE,
    };
    use windows_sys::core::GUID;

    use super::ndr::{FORMAT_STRING_OFFSETS, PROC_FORMAT_STRING, TYPE_FORMAT_STRING};

    /// Interface UUID on `ncalrpc:[DaxRpcEndpoint]` (`DAX3API.exe`,
    /// `RpcServerRegisterIf3`, flags `AUTOLISTEN | ALLOW_LOCAL_ONLY`).
    const DAX_IF_GUID: GUID = GUID {
        data1: 0x11c6_212d,
        data2: 0xd5f3,
        data3: 0x4500,
        data4: [0xab, 0x16, 0x63, 0x4d, 0xc6, 0x30, 0x37, 0xbe],
    };
    /// NDR v2.0 transfer syntax.
    const NDR_SYNTAX_GUID: GUID = GUID {
        data1: 0x8a88_5d04,
        data2: 0x1ceb,
        data3: 0x11c9,
        data4: [0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60],
    };

    /// Build a NUL-terminated UTF-16 literal at compile time.
    /// Literals used here are ASCII, so one u16 per input byte suffices.
    macro_rules! wide {
        ($s:literal) => {{
            const LEN: usize = $s.len() + 1;
            const fn widen() -> [u16; LEN] {
                let bytes = $s.as_bytes();
                let mut out = [0u16; LEN];
                let mut i = 0;
                while i < bytes.len() {
                    out[i] = bytes[i] as u16;
                    i += 1;
                }
                out
            }
            widen()
        }};
    }

    /// Object UUID offered to the server in the opnum-4 handshake, passed as
    /// a BSTR exactly like the vendor client.
    const OBJECT_UUID: &[u16] = &wide!("{B94BA572-658F-40E6-861D-DFB7BB4001A8}");

    const PROTSEQ_NCALRPC: &[u16] = &wide!("ncalrpc");
    const ENDPOINT_DAX: &[u16] = &wide!("DaxRpcEndpoint");

    const RPC_S_OK: i32 = 0;

    // RPC opnums (nProcNum) exercised by this transport.
    const PROC_OPEN_CONTEXT: u32 = 0;
    const PROC_SPATIAL_SUPPORTED: u32 = 2;
    const PROC_REGISTER_OBJECT: u32 = 4;
    const PROC_GET_DOLBY_ENABLED: u32 = 5;
    const PROC_SET_DOLBY_ENABLED: u32 = 6;
    const PROC_GET_ACTIVE_PROFILE: u32 = 7;
    const PROC_SET_ACTIVE_PROFILE: u32 = 8;
    const PROC_GET_SKU: u32 = 37;

    /// `MIDL_STUB_DESC.pfnAllocate`: NDR engine memory goes through the
    /// process heap (same as the vendor's `midl_allocate` thunk).
    unsafe extern "system" fn rpc_alloc(size: usize) -> *mut c_void {
        unsafe { HeapAlloc(GetProcessHeap(), 0, size) }
    }

    unsafe extern "system" fn rpc_free(ptr: *mut c_void) {
        unsafe {
            let _ = HeapFree(GetProcessHeap(), 0, ptr);
        }
    }

    // ---------------------------------------------------------------
    // USER_MARSHAL_ROUTINE_QUADRUPLE for FC_USER_MARSHAL params.
    //
    // Opnums 4 and 37 marshal BSTR through the user-marshal quad table
    // (type-format entry `b4 83` = FC_USER_MARSHAL, quad index 0). The
    // vendor table points at oleaut32's BSTR_User* exports (ordinal
    // 283/284/285/286 = Size/Marshal/Unmarshal/Free); windows-sys binds the
    // same routines with COM-shaped signatures, so tiny adapters adjust the
    // parameter types. oleaut32.dll is a Windows system component.
    // ---------------------------------------------------------------

    unsafe extern "system" fn bstr_size(flags: *mut u32, offset: u32, bstr: *mut c_void) -> u32 {
        unsafe { BSTR_UserSize(flags.cast_const(), offset, bstr.cast::<*const u16>()) }
    }

    unsafe extern "system" fn bstr_marshal(
        flags: *mut u32,
        buffer: *mut u8,
        bstr: *mut c_void,
    ) -> *mut u8 {
        unsafe { BSTR_UserMarshal(flags.cast_const(), buffer, bstr.cast::<*const u16>()) }
    }

    unsafe extern "system" fn bstr_unmarshal(
        flags: *mut u32,
        buffer: *mut u8,
        bstr: *mut c_void,
    ) -> *mut u8 {
        unsafe { BSTR_UserUnmarshal(flags.cast_const(), buffer, bstr.cast::<*const u16>()) }
    }

    unsafe extern "system" fn bstr_free(flags: *mut u32, bstr: *mut c_void) {
        unsafe { BSTR_UserFree(flags.cast_const(), bstr.cast::<*const u16>()) }
    }

    /// Quad table the NDR interpreter indexes via the FC_USER_MARSHAL
    /// type-format entry (index 0 = BSTR, mirroring the vendor table).
    static USER_MARSHAL_QUADS: [USER_MARSHAL_ROUTINE_QUADRUPLE; 1] =
        [USER_MARSHAL_ROUTINE_QUADRUPLE {
            pfnBufferSize: Some(bstr_size),
            pfnMarshall: Some(bstr_marshal),
            pfnUnmarshall: Some(bstr_unmarshal),
            pfnFree: Some(bstr_free),
        }];

    /// Everything the NDR interpreter needs, boxed so field addresses stay
    /// stable across `NativeDolbyRpc` moves (mirroring the vendor impl's
    /// `inner` block: `[0]` binding, `[8]` context token).
    #[repr(C)]
    pub(super) struct DaxMeta {
        /// `RPC_BINDING_HANDLE` from `RpcBindingFromStringBindingW`.
        binding: *mut c_void,
        /// Session context token produced by opnum 0; sized generously —
        /// `NDR_CCONTEXT` internals are opaque.
        context: [usize; 4],
        /// Cell `StubDesc.ImplicitHandleInfo.pPrimitiveHandle` points at.
        /// The vendor keeps a static default cell; we keep it updated so an
        /// implicit handle fetch always sees the live binding.
        implicit_handle: *mut c_void,
        pub(super) syntax_info: [MIDL_SYNTAX_INFO; 1],
        pub(super) proxy_info: MIDL_STUBLESS_PROXY_INFO,
        pub(super) client_interface: RPC_CLIENT_INTERFACE,
        pub(super) stub_desc: MIDL_STUB_DESC,
        /// `RPC_CLIENT_INTERFACE.InterpreterInfo` is dereferenced by
        /// `NdrClientCall3` when the OLE flags (0x06000000) are set: it
        /// reads `nCount`/`pSyntaxInfo` at offsets +0x30/+0x38 and walks
        /// the transfer-syntax table. Mirrors the vendor's
        /// `MIDL_SERVER_INFO`-shaped block.
        server_info: MIDL_SERVER_INFO,
    }

    impl DaxMeta {
        pub(super) fn new() -> Box<Self> {
            let mut meta: Box<Self> = unsafe { Box::new_zeroed().assume_init() };

            // Single transfer syntax: NDR v2.0. The vendor descriptor also
            // declares NDR64, but this client carries only the NDR32
            // format tables, so it advertises one syntax (see the
            // `RPC_CLIENT_INTERFACE::Flags` note below).
            meta.syntax_info = [MIDL_SYNTAX_INFO {
                TransferSyntax: RPC_SYNTAX_IDENTIFIER {
                    SyntaxGUID: NDR_SYNTAX_GUID,
                    SyntaxVersion: RPC_VERSION {
                        MajorVersion: 2,
                        MinorVersion: 0,
                    },
                },
                DispatchTable: ptr::null_mut(),
                ProcString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
                FmtStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
                TypeString: TYPE_FORMAT_STRING.as_ptr().cast_mut(),
                aUserMarshalQuadruple: USER_MARSHAL_QUADS.as_ptr().cast(),
                pMethodProperties: ptr::null(),
                pReserved2: 0,
            }];
            meta.client_interface = RPC_CLIENT_INTERFACE {
                Length: 0x60,
                InterfaceId: RPC_SYNTAX_IDENTIFIER {
                    SyntaxGUID: DAX_IF_GUID,
                    SyntaxVersion: RPC_VERSION {
                        MajorVersion: 1,
                        MinorVersion: 0,
                    },
                },
                TransferSyntax: RPC_SYNTAX_IDENTIFIER {
                    SyntaxGUID: NDR_SYNTAX_GUID,
                    SyntaxVersion: RPC_VERSION {
                        MajorVersion: 2,
                        MinorVersion: 0,
                    },
                },
                DispatchTable: ptr::null_mut(),
                RpcProtseqEndpointCount: 0,
                RpcProtseqEndpoint: ptr::null_mut(),
                Reserved: 0,
                InterpreterInfo: ptr::null(),
                // The vendor descriptor sets 0x0600_0000 (multi-syntax /
                // OLE bits). Those bits make rpcrt4 walk the whole
                // transfer-syntax table and pick NDR64, which needs the
                // vendor's NDR64 format tables. This client only carries
                // the NDR v2.0 tables (every opnum here marshals NDR32, the
                // same bytes the vendor client emits), so the interface
                // advertises a single syntax: the negotiated transfer
                // syntax is then always NDR v2.0 and the copied proc/type
                // strings are the ones the interpreter uses. Verified live
                // against DAX3API (see docs/RE-interfaces.md §12).
                Flags: 0,
            };
            meta.stub_desc = MIDL_STUB_DESC {
                RpcInterfaceInformation: ptr::addr_of_mut!(meta.client_interface).cast(),
                pfnAllocate: Some(rpc_alloc),
                pfnFree: Some(rpc_free),
                IMPLICIT_HANDLE_INFO: MIDL_STUB_DESC_0 {
                    pPrimitiveHandle: ptr::addr_of_mut!(meta.implicit_handle),
                },
                apfnNdrRundownRoutines: ptr::null(),
                aGenericBindingRoutinePairs: ptr::null(),
                apfnExprEval: ptr::null(),
                aXmitQuintuple: ptr::null(),
                pFormatTypes: TYPE_FORMAT_STRING.as_ptr(),
                fCheckBounds: 1,
                Version: 0x0006_0001,
                pMallocFreeStruct: ptr::null_mut(),
                MIDLVersion: 0x0801_0274,
                CommFaultOffsets: ptr::null(),
                aUserMarshalQuadruple: USER_MARSHAL_QUADS.as_ptr(),
                NotifyRoutineTable: ptr::null(),
                mFlags: 0x0200_0001,
                CsRoutineTables: ptr::null(),
                ProxyServerInfo: ptr::null_mut(),
                pExprInfo: ptr::null(),
            };
            meta.proxy_info = MIDL_STUBLESS_PROXY_INFO {
                pStubDesc: ptr::addr_of_mut!(meta.stub_desc),
                ProcFormatString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
                FormatStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
                pTransferSyntax: ptr::addr_of_mut!(meta.syntax_info[0].TransferSyntax),
                // Only NDR v2.0 is offered: the vendor also declares an
                // NDR64 entry, but when both are present the runtime can
                // negotiate NDR64 and then needs the NDR64 format tables.
                // Every proc here marshals NDR32 (same bytes as the vendor
                // client emits), so a single-syntax table keeps the
                // negotiated transfer syntax deterministic.
                nCount: 1,
                pSyntaxInfo: ptr::addr_of_mut!(meta.syntax_info[0]),
            };
            // Vendor parity: the interface's InterpreterInfo is a
            // MIDL_SERVER_INFO-shaped block. The client call path only
            // reads nCount/pSyntaxInfo + each entry's TransferSyntax, but
            // every field is populated with the same blobs the vendor
            // exposes so deeper reads stay consistent.
            meta.server_info = MIDL_SERVER_INFO {
                pStubDesc: ptr::addr_of_mut!(meta.stub_desc),
                DispatchTable: ptr::null(),
                ProcString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
                FmtStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
                ThunkTable: ptr::null(),
                pTransferSyntax: ptr::addr_of_mut!(meta.syntax_info[0].TransferSyntax),
                nCount: 1,
                pSyntaxInfo: ptr::addr_of_mut!(meta.syntax_info[0]),
            };
            meta.client_interface.InterpreterInfo = ptr::addr_of_mut!(meta.server_info).cast();
            // Vendor parity: StubDesc.ProxyServerInfo points back at the
            // proxy info block (the interpreter dereferences it for the
            // notify/callback fields baked into the proc format strings).
            meta.stub_desc.ProxyServerInfo = ptr::addr_of_mut!(meta.proxy_info).cast();
            meta
        }
    }

    // ------------------------------------------------------------------
    // SEH guard: rpcrt4 signals transport failures by raising
    // RPC_S_*/EPT_S_* exceptions (the vendor client uses __try/__except).
    // A first-chance vectored handler restores the context captured just
    // before each call, turning the exception into a plain error code.
    // Non-RPC exceptions (fatal bugs) are never swallowed.
    // ------------------------------------------------------------------

    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
    const CONTEXT_ALL: u32 = 0x0010_001f;

    thread_local! {
        /// 0 = disarmed, 1 = inside a guarded call, 2 = guard already fired.
        static GUARD_STATE: Cell<u8> = const { Cell::new(0) };
        static GUARD_CTX: Cell<*mut CONTEXT> = const { Cell::new(ptr::null_mut()) };
        static GUARD_CODE: Cell<i32> = const { Cell::new(0) };
    }

    unsafe extern "system" fn rpc_exception_filter(info: *mut EXCEPTION_POINTERS) -> i32 {
        unsafe {
            if GUARD_STATE.get() != 1 || info.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let record = (*info).ExceptionRecord;
            if record.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let code = (*record).ExceptionCode;
            if !is_rpc_dispatch_code(code) {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            let ctx = GUARD_CTX.get();
            if ctx.is_null() {
                return EXCEPTION_CONTINUE_SEARCH;
            }
            GUARD_CODE.set(code);
            GUARD_STATE.set(2);
            // Resume after the RtlCaptureContext call site; this never
            // returns to the handler.
            RtlRestoreContext(ctx, record);
            EXCEPTION_CONTINUE_EXECUTION
        }
    }

    /// Whether `code` is an RPC-runtime dispatch failure (Win32 `RPC_S_*` /
    /// `EPT_S_*` space 0x600–0x7ff, or the `HRESULT_FROM_WIN32` /
    /// `FACILITY_RPC` forms). Anything else stays fatal.
    pub(super) fn is_rpc_dispatch_code(code: i32) -> bool {
        let code = code as u32;
        if (0x0600..=0x07ff).contains(&code) {
            return true;
        }
        if (code & 0xffff_0000) == 0x8007_0000 {
            return (0x0600..=0x07ff).contains(&(code & 0xffff));
        }
        (code & 0xffff_0000) == 0x8004_0000
    }

    fn install_exception_handler() {
        static HANDLER: LazyLock<()> = LazyLock::new(|| unsafe {
            let _ = AddVectoredExceptionHandler(1, Some(rpc_exception_filter));
        });
        LazyLock::force(&HANDLER);
    }

    /// Run `call`, returning `Err(code)` if rpcrt4 raises an RPC status
    /// exception. The `ctx` snapshot makes resumption land right after the
    /// `RtlCaptureContext` call — where `GUARD_STATE` is 2 — so the closure
    /// runs exactly once.
    #[inline(never)]
    fn rpc_guarded(call: impl FnOnce()) -> Result<()> {
        install_exception_handler();
        unsafe {
            let mut ctx = MaybeUninit::<CONTEXT>::zeroed();
            (*ctx.as_mut_ptr()).ContextFlags = CONTEXT_ALL;
            GUARD_CTX.set(ctx.as_mut_ptr());
            GUARD_STATE.set(1);
            RtlCaptureContext(ctx.as_mut_ptr());
            if GUARD_STATE.get() == 1 {
                call();
                GUARD_STATE.set(0);
                GUARD_CTX.set(ptr::null_mut());
                return Ok(());
            }
            let code = GUARD_CODE.get();
            GUARD_STATE.set(0);
            GUARD_CTX.set(ptr::null_mut());
            Err(LctrlError::ChannelUnavailable {
                channel: format!("DAX RPC raised exception 0x{code:08x}"),
            })
        }
    }

    fn rpc_status(what: &str, status: i32) -> Result<()> {
        if status == RPC_S_OK {
            Ok(())
        } else {
            Err(LctrlError::ChannelUnavailable {
                channel: format!("{what} failed with RPC status {status}"),
            })
        }
    }

    /// Pure-Rust transport for `ncalrpc:[DaxRpcEndpoint]`.
    ///
    /// Owns one session binding plus the opnum-0 session context. Mirrors
    /// the vendor client's `Initialize` sequence; no Lenovo-shipped binary
    /// is loaded or copied.
    pub(super) struct DaxSession {
        meta: Box<DaxMeta>,
    }

    impl DaxSession {
        /// Compose the ncalrpc binding, open the session context (opnum 0)
        /// and register the object UUID (opnum 4).
        pub(super) fn connect() -> Result<Self> {
            let mut meta = DaxMeta::new();
            unsafe {
                let mut string_binding: *mut u16 = ptr::null_mut();
                rpc_status(
                    "RpcStringBindingComposeW(ncalrpc:[DaxRpcEndpoint])",
                    RpcStringBindingComposeW(
                        ptr::null(),
                        PROTSEQ_NCALRPC.as_ptr(),
                        ptr::null(),
                        ENDPOINT_DAX.as_ptr(),
                        ptr::null(),
                        &mut string_binding,
                    ),
                )?;
                let status =
                    RpcBindingFromStringBindingW(string_binding, ptr::addr_of_mut!(meta.binding));
                let _ = RpcStringFreeW(&mut string_binding);
                rpc_status(
                    "RpcBindingFromStringBindingW(ncalrpc:[DaxRpcEndpoint])",
                    status,
                )?;

                // Implicit-handle cell (safety net; every proc used here
                // passes the handle/context as an argument).
                meta.implicit_handle = meta.binding;

                let binding = meta.binding;
                let context = ptr::addr_of_mut!(meta.context);
                rpc_guarded(|| {
                    NdrClientCall3(
                        ptr::addr_of_mut!(meta.proxy_info),
                        PROC_OPEN_CONTEXT,
                        ptr::null_mut(),
                        binding as usize,
                        context as usize,
                    );
                })?;

                // Object-UUID handshake (vendor opnum 4). The vendor ignores
                // the u16 result; a transport failure is still reported.
                let obj = SysAllocString(OBJECT_UUID.as_ptr());
                let mut registered: u16 = 0;
                let context0 = (*context)[0];
                rpc_guarded(|| {
                    NdrClientCall3(
                        ptr::addr_of_mut!(meta.proxy_info),
                        PROC_REGISTER_OBJECT,
                        ptr::null_mut(),
                        context0,
                        obj as usize,
                        ptr::addr_of_mut!(registered) as usize,
                        obj as usize,
                    );
                })?;
                SysFreeString(obj);
            }
            Ok(Self { meta })
        }

        fn context0(&self) -> usize {
            self.meta.context[0]
        }

        fn proxy_info(&self) -> *mut MIDL_STUBLESS_PROXY_INFO {
            ptr::addr_of!(self.meta.proxy_info).cast_mut()
        }

        pub(super) fn dolby_enabled(&self) -> Result<u16> {
            unsafe {
                let mut enabled: u16 = 0;
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_GET_DOLBY_ENABLED,
                        ptr::null_mut(),
                        ctx,
                        ptr::addr_of_mut!(enabled) as usize,
                    );
                })?;
                Ok(enabled)
            }
        }

        pub(super) fn set_dolby_enabled(&self, enabled: bool) -> Result<()> {
            unsafe {
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_SET_DOLBY_ENABLED,
                        ptr::null_mut(),
                        ctx,
                        usize::from(enabled),
                    );
                })
            }
        }

        pub(super) fn active_profile(&self) -> Result<u32> {
            unsafe {
                let mut profile: u32 = 0;
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_GET_ACTIVE_PROFILE,
                        ptr::null_mut(),
                        ctx,
                        ptr::addr_of_mut!(profile) as usize,
                    );
                })?;
                Ok(profile)
            }
        }

        pub(super) fn set_active_profile(&self, index: u32) -> Result<()> {
            unsafe {
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_SET_ACTIVE_PROFILE,
                        ptr::null_mut(),
                        ctx,
                        index as usize,
                    );
                })
            }
        }

        /// Vendor semantics: spatial audio is offered when the DAX API level
        /// is above 3 or its build number reaches 0x7853.
        pub(super) fn spatial_supported(&self) -> Result<bool> {
            unsafe {
                let mut level: u32 = 0;
                let mut build: u32 = 0;
                let mut extra_a: u32 = 0;
                let mut extra_b: u32 = 0;
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_SPATIAL_SUPPORTED,
                        ptr::null_mut(),
                        ctx,
                        ptr::addr_of_mut!(level) as usize,
                        ptr::addr_of_mut!(build) as usize,
                        ptr::addr_of_mut!(extra_b) as usize,
                        ptr::addr_of_mut!(extra_a) as usize,
                    );
                })?;
                Ok(level > 3 || build >= 0x7853)
            }
        }

        /// The vendor `GetSKU` export converts the server's SKU-name string
        /// into a success flag (its local copy call only returns 0 on
        /// success, so the export effectively returns 1).
        pub(super) fn sku(&self) -> Result<u32> {
            unsafe {
                let mut name: *mut u16 = ptr::null_mut();
                let ctx = self.context0();
                let pi = self.proxy_info();
                rpc_guarded(|| {
                    NdrClientCall3(
                        pi,
                        PROC_GET_SKU,
                        ptr::null_mut(),
                        ctx,
                        ptr::addr_of_mut!(name) as usize,
                    );
                })?;
                if !name.is_null() {
                    // The out param is a user-marshaled BSTR allocated by
                    // BSTR_UserUnmarshal → freed via SysFreeString.
                    SysFreeString(name.cast_const());
                }
                Ok(1)
            }
        }
    }

    impl Drop for DaxSession {
        fn drop(&mut self) {
            unsafe {
                // Context teardown is server-side; the vendor client never
                // calls a close opnum either. The binding itself is freed.
                let _ = RpcBindingFree(ptr::addr_of_mut!(self.meta.binding));
            }
        }
    }
}

/// Native transport for `ncalrpc:[DaxRpcEndpoint]` using only `rpcrt4.dll`
/// (a Windows system component). Requires the `DolbyDAXAPI` service to be
/// running; no Lenovo-shipped binary is touched.
pub struct NativeDolbyRpc {
    session: rpc::DaxSession,
}

// The session is used through &self; rpcrt4 serializes binding calls
// internally, and the context token is read-only after connect().
unsafe impl Send for NativeDolbyRpc {}
unsafe impl Sync for NativeDolbyRpc {}

impl NativeDolbyRpc {
    /// Open the DAX session: compose the ncalrpc binding, open the context
    /// (opnum 0) and register the object UUID (opnum 4).
    ///
    /// Returns `ChannelUnavailable` when the `DolbyDAXAPI` endpoint cannot be
    /// reached (service stopped or endpoint missing).
    pub fn load() -> Result<Self> {
        Ok(Self {
            session: rpc::DaxSession::connect()?,
        })
    }
}

impl DolbyRpc for NativeDolbyRpc {
    fn dolby_enabled(&self) -> Result<bool> {
        Ok(self.session.dolby_enabled()? != 0)
    }

    fn set_dolby_enabled(&self, enabled: bool) -> Result<()> {
        // The vendor wrapper discards the procedure result; failure is
        // detected by the readback polling in `set_dolby_profile`.
        self.session.set_dolby_enabled(enabled)
    }

    fn active_profile(&self) -> Result<u32> {
        self.session.active_profile()
    }

    fn set_active_profile(&self, index: u32) -> Result<()> {
        // Same contract as the vendor: the procedure result is ignored and
        // `poll_readback` verifies the change.
        self.session.set_active_profile(index)
    }

    fn sku(&self) -> Result<u32> {
        self.session.sku()
    }

    fn spatial_supported(&self) -> Result<bool> {
        self.session.spatial_supported()
    }
}

#[cfg(test)]
mod tests {
    use super::ndr::{FORMAT_STRING_OFFSETS, PROC_FORMAT_STRING, TYPE_FORMAT_STRING};
    use super::rpc::{DaxMeta, is_rpc_dispatch_code};
    use std::mem::align_of;

    /// Every nProcNum used by the transport resolves to a proc string whose
    /// embedded procedure number (offset +6, LE u16) matches — the same
    /// bytes the vendor `NdrClientCall3` consumes.
    #[test]
    fn ndr_format_table_roundtrip() {
        for proc_num in [0u16, 2, 4, 5, 6, 7, 8, 37] {
            let offset = FORMAT_STRING_OFFSETS[proc_num as usize] as usize;
            assert!(offset + 10 <= PROC_FORMAT_STRING.len());
            let embedded = u16::from_le_bytes([
                PROC_FORMAT_STRING[offset + 6],
                PROC_FORMAT_STRING[offset + 7],
            ]);
            assert_eq!(embedded, proc_num, "proc string at +{offset}");
        }
    }

    /// The compact offset table never points outside the copied blob.
    #[test]
    fn format_offsets_within_blob() {
        for (proc_num, &offset) in FORMAT_STRING_OFFSETS.iter().enumerate() {
            let offset = offset as usize;
            if offset == 0 && proc_num != 0 {
                continue; // unused entries share proc 0's slot
            }
            assert!(offset < PROC_FORMAT_STRING.len(), "nProcNum {proc_num}");
        }
    }

    /// Descriptor layout must match the vendor metadata exactly: the boxed
    /// meta block stays 8-byte aligned (the interpreter stores pointers
    /// into it) and the boxed address survives moves.
    #[test]
    fn meta_layout_and_pointer_stability() {
        let meta = DaxMeta::new();
        assert_eq!(align_of::<DaxMeta>(), 8);
        let first = &*meta as *const DaxMeta;
        let moved = meta;
        assert_eq!(first, &*moved as *const DaxMeta);
        // The interpreter resolves handles/strings through these pointers;
        // they must point back into the same box.
        assert_eq!(moved.stub_desc.pFormatTypes, TYPE_FORMAT_STRING.as_ptr());
        assert_eq!(
            moved.proxy_info.ProcFormatString,
            PROC_FORMAT_STRING.as_ptr().cast_mut()
        );
        assert_eq!(
            moved.proxy_info.FormatStringOffset,
            FORMAT_STRING_OFFSETS.as_ptr()
        );
        assert_eq!(
            moved.proxy_info.pStubDesc,
            std::ptr::addr_of!(moved.stub_desc).cast_mut()
        );
        assert_eq!(moved.client_interface.Length, 0x60);
        assert_eq!(moved.stub_desc.MIDLVersion, 0x0801_0274);
        // Single-syntax descriptor: the OLE/multi-syntax bits must stay
        // clear, otherwise rpcrt4 negotiates NDR64 and needs format tables
        // this client does not carry (verified live: see §12).
        assert_eq!(moved.client_interface.Flags, 0);
        assert_eq!(moved.proxy_info.nCount, 1);
        // The BSTR user-marshal quad must be populated or the interpreter
        // cannot marshal opnums 4/37.
        assert!(!moved.stub_desc.aUserMarshalQuadruple.is_null());
        assert!(!moved.syntax_info[0].aUserMarshalQuadruple.is_null());
    }

    /// `stack_size` in each proc header (+8, u16) fixes the parameter slot
    /// count the interpreter consumes. The vararg lists at the call sites
    /// must match these arities.
    #[test]
    fn ndr_proc_arity() {
        let expected = [
            (0u16, 0x10u16), // binding + &context
            (2, 0x28),       // context + 4 out-ptrs
            (4, 0x18),       // context + bstr + &u16
            (5, 0x10),       // context + &u16
            (6, 0x10),       // context + u8
            (7, 0x10),       // context + &u32
            (8, 0x10),       // context + u32
            (37, 0x10),      // context + &BSTR
        ];
        for (proc_num, stack) in expected {
            let offset = FORMAT_STRING_OFFSETS[proc_num as usize] as usize;
            let stack_size = u16::from_le_bytes([
                PROC_FORMAT_STRING[offset + 8],
                PROC_FORMAT_STRING[offset + 9],
            ]);
            assert_eq!(stack_size, stack, "proc {proc_num} stack size");
        }
    }

    /// RPC dispatch codes are converted to errors; unrelated codes stay
    /// fatal and are never swallowed by the guard.
    #[test]
    fn rpc_code_classification() {
        assert!(is_rpc_dispatch_code(0x0000_06ba)); // RPC_S_SERVER_UNAVAILABLE
        assert!(is_rpc_dispatch_code(0x8007_06ba_u32.cast_signed())); // HRESULT_FROM_WIN32 form
        assert!(is_rpc_dispatch_code(0x8004_0111_u32.cast_signed())); // FACILITY_RPC
        assert!(!is_rpc_dispatch_code(0x0000_0005)); // access denied — not RPC
        assert!(!is_rpc_dispatch_code(0xc000_0005_u32.cast_signed())); // AV — must propagate
        assert!(!is_rpc_dispatch_code(0x8000_7005_u32.cast_signed()));
    }
}

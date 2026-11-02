//! Owned client for `ncalrpc:[HumanPresenceDetectionEndpoint]`.
//!
//! Only Windows system libraries are linked. The procedure/type metadata is
//! generated from semantic signatures in `ndr::hpd`; no Lenovo client DLL
//! or copied MIDL descriptor table is loaded. The driver-side HPD server is
//! still required. See `docs/evidence/hpd-rpc-re-20261102.md`.

use std::cell::Cell;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::{LazyLock, Mutex};

use lctrl_core::{LctrlError, Result, SenseDistance, SenseState, SenseWait};
use lctrl_hal::SenseFeature;
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, CONTEXT, EXCEPTION_POINTERS, RtlCaptureContext, RtlRestoreContext,
};
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HeapFree};
use windows_sys::Win32::System::Rpc::{
    MIDL_SERVER_INFO, MIDL_STUB_DESC, MIDL_STUB_DESC_0, MIDL_STUBLESS_PROXY_INFO, MIDL_SYNTAX_INFO,
    NdrClientCall3, RPC_CLIENT_INTERFACE, RPC_SYNTAX_IDENTIFIER, RPC_VERSION, RpcBindingFree,
    RpcBindingFromStringBindingW, RpcStringBindingComposeW, RpcStringFreeW,
};
use windows_sys::core::GUID;

use crate::ndr::hpd::{
    FORMAT_STRING_OFFSETS, PROC_FORMAT_STRING, SETTINGS_SCALARS, TYPE_FORMAT_STRING,
};
use crate::wide::wide;

const INTERFACE: GUID = GUID {
    data1: 0x848a_33ac,
    data2: 0xb5a7,
    data3: 0x4253,
    data4: [0xa5, 0x4f, 0xe0, 0xac, 0x20, 0x64, 0x84, 0x68],
};
const NDR32: GUID = GUID {
    data1: 0x8a88_5d04,
    data2: 0x1ceb,
    data3: 0x11c9,
    data4: [0x9f, 0xe8, 0x08, 0x00, 0x2b, 0x10, 0x48, 0x60],
};
const PROTSEQ: &[u16] = &wide!("ncalrpc");
const ENDPOINT: &[u16] = &wide!("HumanPresenceDetectionEndpoint");

/// Test seam for the recovered settings operations. The native implementation
/// owns a binding/context and serializes its use; it does not register callbacks.
pub trait HpdRpc: Send + Sync {
    fn read_state(&self) -> Result<SenseState>;
    fn set_feature(&self, feature: SenseFeature) -> Result<()>;
}

/// Lazy HPD binding. Constructing the CLI's service graph performs no RPC;
/// only a sensing command opens the context. No automatic call replay occurs.
pub struct NativeHpdRpc {
    session: Mutex<Option<Session>>,
}

impl NativeHpdRpc {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }

    fn with_session<T>(&self, call: impl FnOnce(&mut Session) -> Result<T>) -> Result<T> {
        let mut slot = self
            .session
            .lock()
            .map_err(|_| LctrlError::ChannelUnavailable {
                channel: "HPD RPC session lock is poisoned".into(),
            })?;
        if slot.is_none() {
            *slot = Some(Session::connect()?);
        }
        call(slot.as_mut().expect("HPD session initialized"))
    }

    /// Wire op 4. Kept as a typed native diagnostic getter, not inferred from
    /// the driver file version or the registry's unrelated Version value.
    pub fn version(&self) -> Result<u32> {
        self.with_session(Session::version)
    }

    /// Wire op 5 writes eight bytes (`FC_HYPER`). Preserve all bits even
    /// though the managed client's public capability enum is only 32-bit.
    pub fn capability(&self) -> Result<u64> {
        self.with_session(Session::capability)
    }
}

impl Default for NativeHpdRpc {
    fn default() -> Self {
        Self::new()
    }
}

impl HpdRpc for NativeHpdRpc {
    fn read_state(&self) -> Result<SenseState> {
        self.with_session(|session| {
            let capability = session.capability()?;
            let values = session.settings()?;
            let has = |bit: u64| capability & bit != 0;
            // This is the managed client's nullable projection. Capacity
            // (output 1) is a separate support value, not the op-5 bitset.
            Ok(SenseState {
                source: Some("hpd-rpc"),
                advertised: Some(capability),
                lock_on_leave: has(0x0010).then_some(values[5] != 0),
                leave_distance: has(0x0080)
                    .then(|| SenseDistance::from_raw(values[8]))
                    .flatten(),
                leave_wait: has(0x0020)
                    .then(|| SenseWait::from_raw(values[6]))
                    .flatten(),
                wake_on_approach: has(0x0002).then_some(values[2] != 0),
                approach_distance: has(0x0004)
                    .then(|| SenseDistance::from_raw(values[3]))
                    .flatten(),
                pause_video: has(0x0200).then_some(values[10] != 0),
                // These two legacy settings are not fields of this RPC.
                privacy_alert: None,
                attention_tracking: None,
                capability: Some(values[0]),
                sensor_type: has(0x0001).then_some(values[1]),
                facial_feature_registered: has(0x0400).then_some(values[11] != 0),
            })
        })
    }

    fn set_feature(&self, feature: SenseFeature) -> Result<()> {
        let (op, value) = feature
            .rpc_command()
            .ok_or_else(|| LctrlError::Unsupported {
                feature: format!("HPD RPC has no recovered setter for {feature:?}"),
            })?;
        self.with_session(|session| session.set_value(op, value))
    }
}

unsafe extern "system" fn rpc_alloc(size: usize) -> *mut c_void {
    unsafe { HeapAlloc(GetProcessHeap(), 0, size.max(1)) }
}

unsafe extern "system" fn rpc_free(memory: *mut c_void) {
    if !memory.is_null() {
        unsafe {
            let _ = HeapFree(GetProcessHeap(), 0, memory);
        }
    }
}

/// Self-referential interpreter records live at stable heap addresses.
/// A single NDR32 syntax is offered: advertising NDR64 without its generated
/// metadata would let the runtime select an unsupported interpreter path.
struct Meta {
    binding: *mut c_void,
    implicit_handle: *mut c_void,
    syntax: MIDL_SYNTAX_INFO,
    proxy: MIDL_STUBLESS_PROXY_INFO,
    interface: RPC_CLIENT_INTERFACE,
    stub: MIDL_STUB_DESC,
    server_info: MIDL_SERVER_INFO,
    /// `[out]`/`[in,out]` context-handle storage for opnum 0/1. The
    /// marshaller stores the client-side context bookkeeping into this
    /// buffer through the `[out] context_handle*` descriptor — generously
    /// sized and placed last so any NDR_CCONTEXT growth cannot overwrite
    /// the interpreter metadata above. `context[0]` is the live context
    /// token passed to subsequent opnums, matching the vendor thunk's
    /// `client->context` read.
    context: [u64; 16],
}

impl Meta {
    fn new() -> Box<Self> {
        // Windows RPC metadata records contain only integers and raw pointers.
        let mut meta: Box<Self> = Box::new(unsafe { std::mem::zeroed() });
        meta.syntax = MIDL_SYNTAX_INFO {
            TransferSyntax: syntax(NDR32, 2),
            DispatchTable: ptr::null_mut(),
            ProcString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
            FmtStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
            TypeString: TYPE_FORMAT_STRING.as_ptr().cast_mut(),
            aUserMarshalQuadruple: ptr::null(),
            pMethodProperties: ptr::null(),
            pReserved2: 0,
        };
        meta.interface = RPC_CLIENT_INTERFACE {
            Length: std::mem::size_of::<RPC_CLIENT_INTERFACE>() as u32,
            InterfaceId: syntax(INTERFACE, 1),
            TransferSyntax: syntax(NDR32, 2),
            DispatchTable: ptr::null_mut(),
            RpcProtseqEndpointCount: 0,
            RpcProtseqEndpoint: ptr::null_mut(),
            Reserved: 0,
            InterpreterInfo: ptr::null(),
            Flags: 0,
        };
        meta.stub = MIDL_STUB_DESC {
            RpcInterfaceInformation: ptr::addr_of_mut!(meta.interface).cast(),
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
            MIDLVersion: 0x0801_0000,
            CommFaultOffsets: ptr::null(),
            aUserMarshalQuadruple: ptr::null(),
            NotifyRoutineTable: ptr::null(),
            mFlags: 1,
            CsRoutineTables: ptr::null(),
            ProxyServerInfo: ptr::null_mut(),
            pExprInfo: ptr::null(),
        };
        meta.proxy = MIDL_STUBLESS_PROXY_INFO {
            pStubDesc: ptr::addr_of_mut!(meta.stub),
            ProcFormatString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
            FormatStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
            pTransferSyntax: ptr::addr_of_mut!(meta.syntax.TransferSyntax),
            nCount: 1,
            pSyntaxInfo: ptr::addr_of_mut!(meta.syntax),
        };
        meta.server_info = MIDL_SERVER_INFO {
            pStubDesc: ptr::addr_of_mut!(meta.stub),
            DispatchTable: ptr::null(),
            ProcString: PROC_FORMAT_STRING.as_ptr().cast_mut(),
            FmtStringOffset: FORMAT_STRING_OFFSETS.as_ptr(),
            ThunkTable: ptr::null(),
            pTransferSyntax: ptr::addr_of_mut!(meta.syntax.TransferSyntax),
            nCount: 1,
            pSyntaxInfo: ptr::addr_of_mut!(meta.syntax),
        };
        meta.interface.InterpreterInfo = ptr::addr_of_mut!(meta.server_info).cast();
        meta.stub.ProxyServerInfo = ptr::addr_of_mut!(meta.proxy).cast();
        meta
    }
}

fn syntax(guid: GUID, major: u16) -> RPC_SYNTAX_IDENTIFIER {
    RPC_SYNTAX_IDENTIFIER {
        SyntaxGUID: guid,
        SyntaxVersion: RPC_VERSION {
            MajorVersion: major,
            MinorVersion: 0,
        },
    }
}

struct Session {
    meta: Box<Meta>,
}

// All use is serialized by NativeHpdRpc's mutex. RPC handles are opaque;
// the boxed metadata never moves and no raw pointer escapes this module.
unsafe impl Send for Session {}

impl Session {
    fn connect() -> Result<Self> {
        let mut session = Self { meta: Meta::new() };
        unsafe {
            let mut text = ptr::null_mut();
            binding_status(
                "compose",
                RpcStringBindingComposeW(
                    ptr::null(),
                    PROTSEQ.as_ptr(),
                    ptr::null(),
                    ENDPOINT.as_ptr(),
                    ptr::null(),
                    &mut text,
                ),
            )?;
            let status =
                RpcBindingFromStringBindingW(text, ptr::addr_of_mut!(session.meta.binding));
            let _ = RpcStringFreeW(&mut text);
            binding_status("bind", status)?;
            session.meta.implicit_handle = session.meta.binding;
            let proxy = session.proxy();
            let binding = session.meta.binding as usize;
            let context = ptr::addr_of_mut!(session.meta.context) as usize;
            let status = Cell::new(0isize);
            rpc_guarded("Initialize", || {
                status.set(NdrClientCall3(proxy, 0, ptr::null_mut(), binding, context).Simple);
            })?;
            server_status("Initialize", status.get())?;
            if session.meta.context[0] == 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "HPD RPC Initialize succeeded without a context handle".into(),
                });
            }
        }
        Ok(session)
    }

    fn proxy(&self) -> *mut MIDL_STUBLESS_PROXY_INFO {
        ptr::addr_of!(self.meta.proxy).cast_mut()
    }

    fn version(&mut self) -> Result<u32> {
        let mut value = 0u32;
        let status = Cell::new(0isize);
        let proxy = self.proxy();
        let context = self.meta.context[0] as usize;
        rpc_guarded("GetVersion", || unsafe {
            status.set(
                NdrClientCall3(
                    proxy,
                    4,
                    ptr::null_mut(),
                    context,
                    ptr::addr_of_mut!(value) as usize,
                )
                .Simple,
            );
        })?;
        server_status("GetVersion", status.get())?;
        Ok(value)
    }

    fn capability(&mut self) -> Result<u64> {
        let mut value = 0u64;
        let status = Cell::new(0isize);
        let proxy = self.proxy();
        let context = self.meta.context[0] as usize;
        rpc_guarded("GetCapability", || unsafe {
            status.set(
                NdrClientCall3(
                    proxy,
                    5,
                    ptr::null_mut(),
                    context,
                    ptr::addr_of_mut!(value) as usize,
                )
                .Simple,
            );
        })?;
        server_status("GetCapability", status.get())?;
        Ok(value)
    }

    fn settings(&mut self) -> Result<[u32; SETTINGS_SCALARS]> {
        let mut values = [0u32; SETTINGS_SCALARS];
        let mut array = AllocatedWchars(ptr::null_mut());
        let status = Cell::new(0isize);
        let proxy = self.proxy();
        let context = self.meta.context[0] as usize;
        let out = values.as_mut_ptr();
        rpc_guarded("GetAllSetting", || unsafe {
            status.set(
                NdrClientCall3(
                    proxy,
                    6,
                    ptr::null_mut(),
                    context,
                    out as usize,
                    out.add(1) as usize,
                    out.add(2) as usize,
                    out.add(3) as usize,
                    out.add(4) as usize,
                    out.add(5) as usize,
                    out.add(6) as usize,
                    out.add(7) as usize,
                    out.add(8) as usize,
                    out.add(9) as usize,
                    out.add(10) as usize,
                    out.add(11) as usize,
                    out.add(12) as usize,
                    out.add(13) as usize,
                    out.add(14) as usize,
                    ptr::addr_of_mut!(array.0) as usize,
                )
                .Simple,
            );
        })?;
        server_status("GetAllSetting", status.get())?;
        // The extra wchar array is not part of the public settings model.
        // NDR validates size_is(*values[14]); its MIDL allocation is freed
        // on both success and failure by AllocatedWchars, without reading it.
        Ok(values)
    }

    fn set_value(&mut self, op: u32, value: u32) -> Result<()> {
        let operation = match op {
            8 => "SetApproachEnabled",
            9 => "SetApproachDistance",
            11 => "SetPresenceLeaveEnabled",
            12 => "SetPresenceLeaveWaitTime",
            14 => "SetPresenceLeaveDistance",
            16 => "SetVideoAutoPauseResume",
            _ => unreachable!("only typed SenseFeature RPC operations reach this method"),
        };
        let status = Cell::new(0isize);
        let proxy = self.proxy();
        let context = self.meta.context[0] as usize;
        rpc_guarded(operation, || unsafe {
            status.set(NdrClientCall3(proxy, op, ptr::null_mut(), context, value as usize).Simple);
        })?;
        server_status(operation, status.get())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if self.meta.context[0] != 0 {
                let proxy = self.proxy();
                let context = ptr::addr_of_mut!(self.meta.context) as usize;
                // Like the native client's Close, cleanup must not replace
                // a primary operation failure. Binding release still runs.
                let _ = rpc_guarded("ClientClose", || {
                    let _ = NdrClientCall3(proxy, 1, ptr::null_mut(), context);
                });
            }
            if !self.meta.binding.is_null() {
                let _ = RpcBindingFree(ptr::addr_of_mut!(self.meta.binding));
            }
        }
    }
}

struct AllocatedWchars(*mut u16);

impl Drop for AllocatedWchars {
    fn drop(&mut self) {
        unsafe {
            rpc_free(self.0.cast());
        }
    }
}

fn binding_status(operation: &str, status: i32) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(LctrlError::ChannelUnavailable {
            channel: format!("HPD RPC {operation} failed with RPC status {status}"),
        })
    }
}

fn server_status(operation: &str, status: isize) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        // The managed client throws Win32Exception((int)status), not an
        // HRESULT exception. Preserve the complete wire return in diagnostics.
        Err(LctrlError::FirmwareRejected {
            detail: format!(
                "HPD RPC {operation} returned Win32 status {} (wire 0x{:016x})",
                status as u32, status as u64,
            ),
        })
    }
}

// rpcrt4 reports dispatch failures with SEH. This follows the existing DAX
// RtlCaptureContext/RtlRestoreContext guard, catching RPC status exceptions
// only; access violations and other programming faults are not swallowed.
const CONTEXT_ALL: u32 = 0x0010_001f;
thread_local! {
    static GUARD_STATE: Cell<u8> = const { Cell::new(0) };
    static GUARD_CONTEXT: Cell<*mut CONTEXT> = const { Cell::new(ptr::null_mut()) };
    static GUARD_CODE: Cell<i32> = const { Cell::new(0) };
}

fn is_rpc_exception(code: i32) -> bool {
    let code = code as u32;
    matches!(code, 5 | 6 | 8 | 14 | 87)
        || (0x0600..=0x07ff).contains(&code)
        || ((code & 0xffff_0000) == 0x8007_0000 && (0x0600..=0x07ff).contains(&(code & 0xffff)))
        || (code & 0xffff_0000) == 0x8001_0000
}

unsafe extern "system" fn rpc_exception_filter(info: *mut EXCEPTION_POINTERS) -> i32 {
    unsafe {
        if GUARD_STATE.get() != 1 || info.is_null() || (*info).ExceptionRecord.is_null() {
            return 0;
        }
        let record = (*info).ExceptionRecord;
        let code = (*record).ExceptionCode;
        let context = GUARD_CONTEXT.get();
        if !is_rpc_exception(code) || context.is_null() {
            return 0;
        }
        GUARD_CODE.set(code);
        GUARD_STATE.set(2);
        RtlRestoreContext(context, record);
        -1
    }
}

#[inline(never)]
fn rpc_guarded(operation: &str, call: impl FnOnce()) -> Result<()> {
    static HANDLER: LazyLock<usize> = LazyLock::new(|| unsafe {
        AddVectoredExceptionHandler(1, Some(rpc_exception_filter)) as usize
    });
    if *HANDLER == 0 {
        return Err(LctrlError::ChannelUnavailable {
            channel: "HPD RPC exception handler could not be installed".into(),
        });
    }
    unsafe {
        // RtlCaptureContext's body uses `fxsave [rcx+0x100]`, so the buffer
        // must be 16-byte aligned. windows-sys' x64 CONTEXT is `#[repr(C)]`
        // without an explicit alignment (the ABI type is DECLSPEC_ALIGN(16)),
        // so a plain stack MaybeUninit can land at an 8-aligned address and
        // crash inside ntdll before the guarded call runs (observed as
        // 0xC0000005 at ntdll!fxsave on the target). Force the alignment.
        #[repr(C, align(16))]
        struct AlignedContext(CONTEXT);

        let mut context = MaybeUninit::<AlignedContext>::zeroed();
        (*context.as_mut_ptr()).0.ContextFlags = CONTEXT_ALL;
        GUARD_CONTEXT.set(context.as_mut_ptr().cast());
        GUARD_CODE.set(0);
        GUARD_STATE.set(1);
        RtlCaptureContext(context.as_mut_ptr().cast());
        if GUARD_STATE.get() == 1 {
            call();
            GUARD_STATE.set(0);
            GUARD_CONTEXT.set(ptr::null_mut());
            return Ok(());
        }
        let code = GUARD_CODE.get();
        GUARD_STATE.set(0);
        GUARD_CONTEXT.set(ptr::null_mut());
        Err(LctrlError::ChannelUnavailable {
            channel: format!("HPD RPC {operation} raised exception 0x{code:08x}"),
        })
    }
}

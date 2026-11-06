//! Windows hardware radio channel.
//!
//! Owner recovery, live receipts and rejected read-backs: `docs/evidence/re-radio-20261105.md`.
//! In short: the owner is the WinRT Radio API (`Windows.Devices.Radios.Radio`) over the operating
//! system's `RadioManagement` plane; there is no Lenovo radio WMI class on this SKU and the
//! EnergyDrv surface is battery/charge only. Wi-Fi additionally has a documented per-PHY
//! soft/hard switch pair through the WLAN API, which is the only place that distinction exists.
//!
//! Everything here is fail-closed: an unexpected `HRESULT`, a returned object whose runtime class
//! is not the documented one, an unknown asynchronous status, or a radio-state buffer that does not
//! match its own length all surface as `Unsupported`/`ChannelUnavailable` instead of a value read
//! from a slot that was assumed to be something else.

use std::ffi::c_void;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};

use lctrl_core::{ApplyMode, LctrlError, Result};
use lctrl_hal::{
    InterfaceKind, NetworkInterfaceFacts, PhyRadioState, RadioAccess, RadioControl, RadioKind,
    RadioReport, RadioState, RadioStatus, RadioSwitch, RadioTransition, WlanLinkState,
    WlanRadioDetail, radio_session_cut, remote_session_from_env, validate_requested_state,
};
use windows_sys::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, HANDLE};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GetAdaptersAddresses, IF_TYPE_IEEE80211,
    IF_TYPE_WWANPP, IF_TYPE_WWANPP2, IP_ADAPTER_ADDRESSES_LH,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::NetworkManagement::WiFi::{
    WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WlanCloseHandle, WlanEnumInterfaces,
    WlanFreeMemory, WlanOpenHandle, WlanQueryInterface, dot11_radio_state_off, dot11_radio_state_on,
    dot11_radio_state_unknown, wlan_intf_opcode_interface_state, wlan_intf_opcode_radio_state,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKET_ADDRESS};
use windows_sys::core::GUID;

/// `WLAN_API_VERSION_2_0`: the negotiated client version the live probe returned (2).
const WLAN_CLIENT_VERSION: u32 = 2;
/// `WLAN_RADIO_STATE` holds at most 64 PHYs; anything larger is a malformed buffer.
const MAX_PHYS: u32 = 64;
/// A machine cannot have more radios than this; a larger count means the wrong slot was read.
const MAX_RADIOS: u32 = 64;
const MAX_WLAN_INTERFACES: u32 = 64;
const MAX_ADAPTERS: usize = 256;
const MAX_UNICAST_ADDRESSES: usize = 32;

/// Bluetooth bring-up took ≈ 11.7 s on the target, so the write window is deliberately generous.
const SET_STATE_DEADLINE: Duration = Duration::from_secs(60);
const ENUMERATE_DEADLINE: Duration = Duration::from_secs(15);
const ACCESS_DEADLINE: Duration = Duration::from_secs(10);
/// Read-back may follow a long transition; poll long enough to see it land.
const READBACK_DEADLINE: Duration = Duration::from_secs(10);
const ASYNC_POLL_INTERVAL: Duration = Duration::from_millis(10);
const READBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The Windows radio service. It holds no state: every call opens and releases its own WinRT
/// apartment, factory and interfaces.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRadio;

impl NativeRadio {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl RadioControl for NativeRadio {
    fn radio_status(&self) -> Result<RadioReport> {
        let session = winrt::Session::open()?;
        let radios = session.radios()?;
        let mut statuses = Vec::with_capacity(radios.len());
        for radio in &radios {
            statuses.push(RadioStatus {
                kind: radio.kind()?,
                name: radio.name().ok(),
                state: radio.state()?,
                wlan_interfaces: Vec::new(),
            });
        }
        // The per-PHY soft/hard switches exist only for Wi-Fi; query them only when the platform
        // actually enumerated a Wi-Fi radio, so a machine without one does not need the service.
        if statuses.iter().any(|status| status.kind == RadioKind::Wifi) {
            let wlan = wlan::interfaces()?;
            for status in &mut statuses {
                if status.kind == RadioKind::Wifi {
                    status.wlan_interfaces = wlan.clone();
                }
            }
        }
        let absent_kinds = RadioKind::SELECTABLE
            .iter()
            .copied()
            .filter(|kind| !statuses.iter().any(|status| status.kind == *kind))
            .collect();
        Ok(RadioReport {
            access: session.access(),
            radios: statuses,
            absent_kinds,
        })
    }

    fn radio_set_state(
        &self,
        kind: RadioKind,
        state: RadioState,
        allow_session_cut: bool,
        apply: ApplyMode,
    ) -> Result<RadioTransition> {
        let requested = validate_requested_state(state)?;
        // The guard runs before any controller work: switching a radio off can sever the session
        // that issued the command, and a machine that loses its only path cannot be repaired by
        // the same command.
        if requested == RadioState::Off && !allow_session_cut {
            if let Some(session) = remote_session_from_env() {
                let interfaces = adapters::list()?;
                if let Some(refusal) = radio_session_cut(kind, &interfaces, Some(&session)) {
                    return Err(refusal.to_error());
                }
            }
        }

        let session = winrt::Session::open()?;
        ensure_allowed(session.access())?;
        let radios = session.radios()?;
        let mut target = None;
        for radio in &radios {
            if radio.kind()? == kind {
                target = Some(radio);
                break;
            }
        }
        let Some(radio) = target else {
            return Err(LctrlError::Unsupported {
                feature: format!(
                    "radio {} (no such radio is enumerated on this machine)",
                    kind.as_str()
                ),
            });
        };

        let name = radio.name().ok();
        let previous = radio.state()?;
        if apply == ApplyMode::DryRun {
            // A dry run observes and plans; it never calls `SetStateAsync`.
            return Ok(RadioTransition {
                mode: ApplyMode::DryRun,
                kind,
                name,
                requested,
                previous,
                observed: None,
                access: session.access(),
                rollback_observed: None,
            });
        }
        if previous == requested {
            // No write is attempted when the observed state already matches: an accepted request
            // that changes nothing is not evidence that a write path works.
            return Ok(RadioTransition {
                mode: ApplyMode::Commit,
                kind,
                name,
                requested,
                previous,
                observed: Some(previous),
                access: session.access(),
                rollback_observed: None,
            });
        }

        let granted = radio.set_state(requested)?;
        ensure_allowed(granted)?;
        let observed = read_back(radio, requested)?;
        if observed == requested {
            return Ok(RadioTransition {
                mode: ApplyMode::Commit,
                kind,
                name,
                requested,
                previous,
                observed: Some(observed),
                access: granted,
                rollback_observed: None,
            });
        }

        // The read-back missed the request: restore the previous state through the same call and
        // report both outcomes instead of claiming a either a success or a silent restore.
        let rollback_observed = radio
            .set_state(previous)
            .and_then(|_| read_back(radio, previous))
            .ok();
        let rollback = match rollback_observed {
            Some(state) if state == previous => "restored",
            Some(_) => "did not reach the previous state",
            None => "failed",
        };
        Err(LctrlError::VerifyMismatch {
            requested: requested.as_str().to_owned(),
            actual: format!(
                "{} (rollback to {} {rollback})",
                observed.as_str(),
                previous.as_str()
            ),
        })
    }
}

fn ensure_allowed(access: RadioAccess) -> Result<()> {
    match access {
        RadioAccess::Allowed => Ok(()),
        RadioAccess::DeniedByUser => Err(LctrlError::PermissionDenied {
            need: "Windows radio access consent (RadioAccessStatus::DeniedByUser); grant radio \
                   access to this application and retry"
                .to_owned(),
        }),
        RadioAccess::DeniedBySystem => Err(LctrlError::Unsupported {
            feature: "radio control (the system denies radio access to this application: \
                      RadioAccessStatus::DeniedBySystem)"
                .to_owned(),
        }),
        RadioAccess::Unspecified => Err(LctrlError::ChannelUnavailable {
            channel: "Windows radio access status was not reported (RadioAccessStatus::Unspecified)"
                .to_owned(),
        }),
    }
}

/// Poll the radio's own state until it reports `expected` or the read-back window closes.
fn read_back(radio: &winrt::Radio, expected: RadioState) -> Result<RadioState> {
    let started = Instant::now();
    loop {
        let observed = radio.state()?;
        if observed == expected || started.elapsed() >= READBACK_DEADLINE {
            return Ok(observed);
        }
        thread::sleep(READBACK_POLL_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// WinRT `Windows.Devices.Radios` (hand-written ABI, minimal vtable surface)
// ---------------------------------------------------------------------------

mod winrt {
    use super::*;

    const RADIO_CLASS: &str = "Windows.Devices.Radios.Radio";
    const RADIO_RUNTIME_CLASS: &str = "Windows.Devices.Radios.Radio";
    /// The collection the platform returns for `GetRadiosAsync`; only the shape is asserted, the
    /// element type is appended by the projection.
    const VECTOR_VIEW_PREFIX: &str = "Windows.Foundation.Collections.IVectorView`1<";

    const RO_INIT_MULTITHREADED: u32 = 1;
    /// The host thread was already initialized into an apartment (observed live on the target).
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

    const IID_IRADIO_STATICS: GUID =
        GUID::from_u128(0x5fb6_a12e_67cb_46ae_aae9_65919f86eff4);
    const IID_IRADIO: GUID = GUID::from_u128(0x2521_18df_b33e_416a_875f_1cf38ae2d83e);
    const IID_IASYNC_INFO: GUID = GUID::from_u128(0x0000_0036_0000_0000_c000_000000000046);

    /// `IAsyncStatus`: Canceled = 2, Completed = 1, Error = 3, Started = 0.
    const ASYNC_STARTED: i32 = 0;
    const ASYNC_COMPLETED: i32 = 1;
    const ASYNC_CANCELED: i32 = 2;
    const ASYNC_ERROR: i32 = 3;

    /// A vtable slot this module does not call. Declared so the slots that are called keep their
    /// documented offset instead of being guessed.
    type Slot = *const c_void;

    #[link(name = "combase")]
    unsafe extern "system" {
        fn RoInitialize(init_type: u32) -> i32;
        fn RoUninitialize();
        fn WindowsCreateString(
            source: *const u16,
            length: u32,
            string: *mut *mut c_void,
        ) -> i32;
        fn WindowsDeleteString(string: *mut c_void) -> i32;
        fn WindowsGetStringRawBuffer(string: *mut c_void, length: *mut u32) -> *const u16;
        fn RoGetActivationFactory(
            class_id: *mut c_void,
            iid: *const GUID,
            factory: *mut *mut c_void,
        ) -> i32;
    }

    /// `IInspectable` base: QueryInterface, AddRef, Release, GetIids, GetRuntimeClassName,
    /// GetTrustLevel. The three slots this module calls are typed; the rest are placeholders.
    #[allow(dead_code)] // placeholder slots exist only to keep the documented offsets
    #[repr(C)]
    struct InspectableVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
        add_ref: Slot,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_iids: Slot,
        get_runtime_class_name: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
        get_trust_level: Slot,
    }

    /// `IRadioStatics` `{5FB6A12E-…}`: GetRadiosAsync, GetDeviceSelector, FromIdAsync,
    /// RequestAccessAsync.
    #[allow(dead_code)] // GetDeviceSelector/FromIdAsync are documented but never called
    #[repr(C)]
    struct RadioStaticsVtbl {
        base: InspectableVtbl,
        get_radios_async: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
        get_device_selector: Slot,
        from_id_async: Slot,
        request_access_async: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    }

    /// `IRadio` `{252118DF-…}`: SetStateAsync, add/remove StateChanged, get_State, get_Name,
    /// get_Kind.
    #[allow(dead_code)] // the StateChanged pair is documented but never subscribed to
    #[repr(C)]
    struct RadioVtbl {
        base: InspectableVtbl,
        set_state_async: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32,
        add_state_changed: Slot,
        remove_state_changed: Slot,
        get_state: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
        get_name: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
        get_kind: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    }

    /// `IAsyncOperation<T>`: SetCompleted, get_Completed, GetResults (`IAsyncInfo` is a separate
    /// interface reached through QueryInterface).
    #[allow(dead_code)] // completion handlers are not installed; status is polled instead
    #[repr(C)]
    struct AsyncOperationVtbl {
        base: InspectableVtbl,
        set_completed: Slot,
        get_completed: Slot,
        get_results: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    }

    /// `IAsyncInfo` `{00000036-0000-0000-C000-000000000046}`: get_Id, get_Status, get_ErrorCode,
    /// Cancel, Close.
    #[allow(dead_code)] // Id/Cancel/Close are documented but never called
    #[repr(C)]
    struct AsyncInfoVtbl {
        base: InspectableVtbl,
        get_id: Slot,
        get_status: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
        get_error_code: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
        cancel: Slot,
        close: Slot,
    }

    /// `IVectorView<T>`: GetAt, get_Size, IndexOf, GetMany (`IIterable` is reached separately).
    #[allow(dead_code)] // IndexOf/GetMany are documented but never called
    #[repr(C)]
    struct VectorViewVtbl {
        base: InspectableVtbl,
        get_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
        get_size: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
        index_of: Slot,
        get_many: Slot,
    }

    /// Borrow an interface's vtable through its documented layout.
    ///
    /// # Safety
    ///
    /// `this` must be a live interface pointer whose vtable really is `T`. Every caller reaches it
    /// from a documented activation path, and the element/collection identity checks in
    /// [`Session::radios`] reject an object whose runtime class is not the documented one before
    /// any typed call is made through it.
    unsafe fn vtable<'a, T>(this: *mut c_void) -> &'a T {
        unsafe { &*(*(this as *const *const T)) }
    }

    fn check(result: i32, context: &'static str) -> Result<()> {
        if result < 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{context} failed with HRESULT 0x{:08X}", result as u32),
            });
        }
        Ok(())
    }

    fn unsupported(detail: impl std::fmt::Display) -> LctrlError {
        LctrlError::Unsupported {
            feature: format!("Windows.Devices.Radios: {detail}"),
        }
    }

    /// An owned COM interface. Released exactly once on drop.
    struct Interface {
        raw: *mut c_void,
    }

    impl Interface {
        fn new(raw: *mut c_void, context: &'static str) -> Result<Self> {
            if raw.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("{context} returned a null interface pointer"),
                });
            }
            Ok(Self { raw })
        }

        fn query(&self, iid: &GUID, context: &'static str) -> Result<Self> {
            let mut raw: *mut c_void = ptr::null_mut();
            let result = unsafe {
                let inspectable = vtable::<InspectableVtbl>(self.raw);
                (inspectable.query_interface)(self.raw, iid, &mut raw)
            };
            check(result, context)?;
            Self::new(raw, context)
        }

        fn runtime_class_name(&self) -> Result<String> {
            let mut string: *mut c_void = ptr::null_mut();
            let result = unsafe {
                let inspectable = vtable::<InspectableVtbl>(self.raw);
                (inspectable.get_runtime_class_name)(self.raw, &mut string)
            };
            check(result, "IInspectable::GetRuntimeClassName")?;
            Ok(HString { raw: string }.to_string())
        }
    }

    impl Drop for Interface {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    let inspectable = vtable::<InspectableVtbl>(self.raw);
                    (inspectable.release)(self.raw);
                }
                self.raw = ptr::null_mut();
            }
        }
    }

    /// An owned `HSTRING`, deleted exactly once on drop.
    struct HString {
        raw: *mut c_void,
    }

    impl HString {
        fn create(value: &str) -> Result<Self> {
            let wide: Vec<u16> = value.encode_utf16().collect();
            let length = u32::try_from(wide.len()).map_err(|_| unsupported("class name too long"))?;
            let mut raw: *mut c_void = ptr::null_mut();
            let result = unsafe { WindowsCreateString(wide.as_ptr(), length, &mut raw) };
            check(result, "WindowsCreateString")?;
            if raw.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WindowsCreateString returned a null HSTRING".to_owned(),
                });
            }
            Ok(Self { raw })
        }

        fn to_string(&self) -> String {
            let mut length = 0u32;
            let buffer = unsafe { WindowsGetStringRawBuffer(self.raw, &mut length) };
            if buffer.is_null() || length == 0 {
                return String::new();
            }
            let units = unsafe { std::slice::from_raw_parts(buffer, length as usize) };
            String::from_utf16_lossy(units)
        }
    }

    impl Drop for HString {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    WindowsDeleteString(self.raw);
                }
                self.raw = ptr::null_mut();
            }
        }
    }

    /// The process-wide Windows Runtime apartment opened for one call.
    struct Apartment {
        owned: bool,
    }

    impl Apartment {
        fn enter() -> Result<Self> {
            let result = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            if result == RPC_E_CHANGED_MODE {
                // The host thread already lives in an apartment (a PowerShell host returns this).
                // Activation works from either apartment, and an apartment this call did not
                // create must not be uninitialized.
                return Ok(Self { owned: false });
            }
            check(result, "RoInitialize")?;
            Ok(Self { owned: true })
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.owned {
                unsafe { RoUninitialize() };
                self.owned = false;
            }
        }
    }

    /// Wait for an `IAsyncOperation` by polling `IAsyncInfo::get_Status`, which is the documented
    /// interface for an operation's status. No completion handler is installed, so no delegate
    /// vtable has to be synthesized.
    fn await_operation(operation: &Interface, deadline: Duration, context: &'static str) -> Result<()> {
        let info = operation.query(&IID_IASYNC_INFO, "QueryInterface(IAsyncInfo)")?;
        let started = Instant::now();
        loop {
            let mut status = -1;
            let result = unsafe {
                let async_info = vtable::<AsyncInfoVtbl>(info.raw);
                (async_info.get_status)(info.raw, &mut status)
            };
            check(result, "IAsyncInfo::get_Status")?;
            match status {
                ASYNC_COMPLETED => return Ok(()),
                ASYNC_CANCELED | ASYNC_ERROR => {
                    let mut code = 0;
                    let error = unsafe {
                        let async_info = vtable::<AsyncInfoVtbl>(info.raw);
                        (async_info.get_error_code)(info.raw, &mut code)
                    };
                    let detail = if error < 0 {
                        format!("status {status}, error code unreadable")
                    } else {
                        format!("status {status}, error code 0x{:08X}", code as u32)
                    };
                    return Err(LctrlError::ChannelUnavailable {
                        channel: format!("{context}: the asynchronous operation ended ({detail})"),
                    });
                }
                ASYNC_STARTED => {}
                other => {
                    return Err(unsupported(format!(
                        "{context}: unknown asynchronous status {other}"
                    )));
                }
            }
            if started.elapsed() >= deadline {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("{context}: the asynchronous operation did not complete in time"),
                });
            }
            thread::sleep(ASYNC_POLL_INTERVAL);
        }
    }

    /// `IAsyncOperation<T>::GetResults` for an interface-typed result.
    fn results_interface(
        operation: &Interface,
        context: &'static str,
    ) -> Result<Interface> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            let async_operation = vtable::<AsyncOperationVtbl>(operation.raw);
            (async_operation.get_results)(
                operation.raw,
                (&mut raw as *mut *mut c_void).cast::<c_void>(),
            )
        };
        check(result, context)?;
        Interface::new(raw, context)
    }

    /// `IAsyncOperation<T>::GetResults` for a 32-bit result (an enum or a status code).
    fn results_i32(operation: &Interface, context: &'static str) -> Result<i32> {
        let mut value = 0i32;
        let result = unsafe {
            let async_operation = vtable::<AsyncOperationVtbl>(operation.raw);
            // `GetResults` writes a 32-bit enum (or status) into this out-parameter.
            (async_operation.get_results)(operation.raw, (&mut value as *mut i32).cast::<c_void>())
        };
        check(result, context)?;
        Ok(value)
    }

    /// One call's WinRT state: the apartment, the `IRadioStatics` factory and the radio consent.
    ///
    /// Field order matters: `factory` is released before the apartment is uninitialized.
    pub(super) struct Session {
        factory: Interface,
        access: RadioAccess,
        _apartment: Apartment,
    }

    impl Session {
        pub(super) fn open() -> Result<Self> {
            let apartment = Apartment::enter()?;
            let class = HString::create(RADIO_CLASS)?;
            let mut raw: *mut c_void = ptr::null_mut();
            let result =
                unsafe { RoGetActivationFactory(class.raw, &IID_IRADIO_STATICS, &mut raw) };
            check(result, "RoGetActivationFactory(Windows.Devices.Radios.Radio)")?;
            let factory = Interface::new(raw, "RoGetActivationFactory")?;
            // The activation factory itself rejects `GetRuntimeClassName` with E_ILLEGAL_METHOD_CALL
            // (observed live), so the factory's identity cannot be asserted this way; the returned
            // radio objects and their collection are asserted instead.
            let access = request_access(&factory)?;
            Ok(Self {
                factory,
                access,
                _apartment: apartment,
            })
        }

        pub(super) const fn access(&self) -> RadioAccess {
            self.access
        }

        pub(super) fn radios(&self) -> Result<Vec<Radio>> {
            let mut operation_raw: *mut c_void = ptr::null_mut();
            let result = unsafe {
                let statics = vtable::<RadioStaticsVtbl>(self.factory.raw);
                (statics.get_radios_async)(self.factory.raw, &mut operation_raw)
            };
            check(result, "IRadioStatics::GetRadiosAsync")?;
            let operation = Interface::new(operation_raw, "IRadioStatics::GetRadiosAsync")?;
            await_operation(&operation, ENUMERATE_DEADLINE, "GetRadiosAsync")?;
            let view = results_interface(&operation, "GetRadiosAsync::GetResults")?;
            let class_name = view.runtime_class_name()?;
            if !class_name.starts_with(VECTOR_VIEW_PREFIX) {
                return Err(unsupported(format!(
                    "GetRadiosAsync returned a collection whose runtime class is \"{class_name}\""
                )));
            }
            let mut count = 0u32;
            let result = unsafe {
                let collection = vtable::<VectorViewVtbl>(view.raw);
                (collection.get_size)(view.raw, &mut count)
            };
            check(result, "IVectorView::get_Size")?;
            if count > MAX_RADIOS {
                return Err(unsupported(format!(
                    "GetRadiosAsync reported {count} radios, which exceeds the {MAX_RADIOS} bound"
                )));
            }
            let mut radios = Vec::with_capacity(count as usize);
            for index in 0..count {
                let mut item_raw: *mut c_void = ptr::null_mut();
                let result = unsafe {
                    let collection = vtable::<VectorViewVtbl>(view.raw);
                    (collection.get_at)(view.raw, index, &mut item_raw)
                };
                check(result, "IVectorView::GetAt")?;
                let item = Interface::new(item_raw, "IVectorView::GetAt")?;
                let class_name = item.runtime_class_name()?;
                if class_name != RADIO_RUNTIME_CLASS {
                    return Err(unsupported(format!(
                        "GetRadiosAsync returned an element whose runtime class is \"{class_name}\""
                    )));
                }
                let radio = item.query(&IID_IRADIO, "QueryInterface(IRadio)")?;
                radios.push(Radio { interface: radio });
            }
            Ok(radios)
        }
    }

    fn request_access(factory: &Interface) -> Result<RadioAccess> {
        let mut operation_raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            let statics = vtable::<RadioStaticsVtbl>(factory.raw);
            (statics.request_access_async)(factory.raw, &mut operation_raw)
        };
        check(result, "IRadioStatics::RequestAccessAsync")?;
        let operation = Interface::new(operation_raw, "IRadioStatics::RequestAccessAsync")?;
        await_operation(&operation, ACCESS_DEADLINE, "RequestAccessAsync")?;
        let value = results_i32(&operation, "RequestAccessAsync::GetResults")?;
        Ok(match value {
            1 => RadioAccess::Allowed,
            2 => RadioAccess::DeniedByUser,
            3 => RadioAccess::DeniedBySystem,
            0 => RadioAccess::Unspecified,
            other => {
                return Err(unsupported(format!(
                    "RequestAccessAsync returned unknown RadioAccessStatus {other}"
                )));
            }
        })
    }

    /// One owned `IRadio`.
    pub(super) struct Radio {
        interface: Interface,
    }

    impl Radio {
        pub(super) fn kind(&self) -> Result<RadioKind> {
            let mut value = 0i32;
            let result = unsafe {
                let radio = vtable::<RadioVtbl>(self.interface.raw);
                (radio.get_kind)(self.interface.raw, &mut value)
            };
            check(result, "IRadio::get_Kind")?;
            Ok(match value {
                1 => RadioKind::Wifi,
                2 => RadioKind::MobileBroadband,
                3 => RadioKind::Bluetooth,
                4 => RadioKind::Fm,
                0 => RadioKind::Other,
                other => {
                    return Err(unsupported(format!(
                        "IRadio::get_Kind returned unknown RadioKind {other}"
                    )));
                }
            })
        }

        pub(super) fn state(&self) -> Result<RadioState> {
            let mut value = 0i32;
            let result = unsafe {
                let radio = vtable::<RadioVtbl>(self.interface.raw);
                (radio.get_state)(self.interface.raw, &mut value)
            };
            check(result, "IRadio::get_State")?;
            Ok(match value {
                1 => RadioState::On,
                2 => RadioState::Off,
                3 => RadioState::Disabled,
                0 => RadioState::Unknown,
                other => {
                    return Err(unsupported(format!(
                        "IRadio::get_State returned unknown RadioState {other}"
                    )));
                }
            })
        }

        pub(super) fn name(&self) -> Result<String> {
            let mut string: *mut c_void = ptr::null_mut();
            let result = unsafe {
                let radio = vtable::<RadioVtbl>(self.interface.raw);
                (radio.get_name)(self.interface.raw, &mut string)
            };
            check(result, "IRadio::get_Name")?;
            Ok(HString { raw: string }.to_string())
        }

        /// Request a state and return the access status the platform granted for this call.
        pub(super) fn set_state(&self, state: RadioState) -> Result<RadioAccess> {
            let value = match state {
                RadioState::On => 1,
                RadioState::Off => 2,
                RadioState::Disabled | RadioState::Unknown => {
                    return Err(LctrlError::InvalidArgument {
                        detail: format!(
                            "radio state \"{}\" cannot be requested",
                            state.as_str()
                        ),
                    });
                }
            };
            let mut operation_raw: *mut c_void = ptr::null_mut();
            let result = unsafe {
                let radio = vtable::<RadioVtbl>(self.interface.raw);
                (radio.set_state_async)(self.interface.raw, value, &mut operation_raw)
            };
            check(result, "IRadio::SetStateAsync")?;
            let operation = Interface::new(operation_raw, "IRadio::SetStateAsync")?;
            await_operation(&operation, SET_STATE_DEADLINE, "SetStateAsync")?;
            let granted = results_i32(&operation, "SetStateAsync::GetResults")?;
            Ok(match granted {
                1 => RadioAccess::Allowed,
                2 => RadioAccess::DeniedByUser,
                3 => RadioAccess::DeniedBySystem,
                0 => RadioAccess::Unspecified,
                other => {
                    return Err(unsupported(format!(
                        "SetStateAsync returned unknown RadioAccessStatus {other}"
                    )));
                }
            })
        }

    }
}

// ---------------------------------------------------------------------------
// WLAN API: documented per-PHY software/hardware radio state
// ---------------------------------------------------------------------------

mod wlan {
    use super::*;

    /// One `WLAN_INTERFACE_INFO` with its radio and link state.
    struct WlanInterface {
        guid: GUID,
        description: String,
        link_state_raw: i32,
        phys: Vec<PhyRadioState>,
    }

    /// An owned WLAN client handle; the service handle is closed exactly once.
    struct Client {
        handle: HANDLE,
    }

    impl Client {
        fn open() -> Result<Self> {
            let mut negotiated = 0u32;
            // `HANDLE` is `*mut c_void` in windows-sys 0.61, not an integer.
            let mut handle: HANDLE = ptr::null_mut();
            let code = unsafe {
                WlanOpenHandle(
                    WLAN_CLIENT_VERSION,
                    ptr::null(),
                    &mut negotiated,
                    &mut handle,
                )
            };
            if code != 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanOpenHandle returned {code}"),
                });
            }
            Ok(Self { handle })
        }

        fn interfaces(&self) -> Result<Vec<WlanInterface>> {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
            let code = unsafe { WlanEnumInterfaces(self.handle, ptr::null(), &mut list) };
            if code != 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanEnumInterfaces returned {code}"),
                });
            }
            if list.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WlanEnumInterfaces returned a null interface list".to_owned(),
                });
            }
            let guard = WlanMemory(list as *const c_void);
            let base = list.cast::<u8>();
            // Safety: the service returned a `WLAN_INTERFACE_INFO_LIST` followed by
            // `dwNumberOfItems` entries; the header is read before the entries.
            let count = unsafe { (*list).dwNumberOfItems };
            if count > MAX_WLAN_INTERFACES {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanEnumInterfaces reported {count} interfaces"),
                });
            }
            let entry_offset = std::mem::offset_of!(WLAN_INTERFACE_INFO_LIST, InterfaceInfo);
            let entry_size = std::mem::size_of::<WLAN_INTERFACE_INFO>();
            let mut interfaces = Vec::with_capacity(count as usize);
            for index in 0..count as usize {
                // Safety: bounded by `count` above; each entry lies inside the service buffer.
                let info = unsafe {
                    let entry = base.add(entry_offset + index * entry_size);
                    &*entry.cast::<WLAN_INTERFACE_INFO>()
                };
                let description = info
                    .strInterfaceDescription
                    .iter()
                    .take_while(|unit| **unit != 0)
                    .copied()
                    .collect::<Vec<u16>>();
                let mut interface = WlanInterface {
                    guid: info.InterfaceGuid,
                    description: String::from_utf16_lossy(&description),
                    link_state_raw: -1,
                    phys: Vec::new(),
                };
                interface.link_state_raw = self.query_i32(&interface.guid, wlan_intf_opcode_interface_state)?;
                interface.phys = self.query_phys(&interface.guid)?;
                interfaces.push(interface);
            }
            drop(guard);
            Ok(interfaces)
        }

        /// Query a 32-bit opcode (only the interface-state opcode uses this path).
        fn query_i32(&self, guid: &GUID, opcode: i32) -> Result<i32> {
            let mut size = 0u32;
            let mut data: *mut c_void = ptr::null_mut();
            let mut value_type = 0i32;
            let code = unsafe {
                WlanQueryInterface(
                    self.handle,
                    guid,
                    opcode,
                    ptr::null(),
                    &mut size,
                    &mut data,
                    &mut value_type,
                )
            };
            if code != 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanQueryInterface(opcode {opcode}) returned {code}"),
                });
            }
            if data.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanQueryInterface(opcode {opcode}) returned no data"),
                });
            }
            let guard = WlanMemory(data as *const c_void);
            if size < 4 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "WlanQueryInterface(opcode {opcode}) returned {size} bytes, shorter than one value"
                    ),
                });
            }
            // Safety: at least four bytes were returned for this opcode.
            let value = unsafe { ptr::read_unaligned(data.cast::<i32>()) };
            drop(guard);
            Ok(value)
        }

        /// Query `wlan_intf_opcode_radio_state` and decode its per-PHY switch states.
        ///
        /// The service returns the fixed-size `WLAN_RADIO_STATE` (4 + 64 * 12 bytes) and states the
        /// real PHY count in `dwNumberOfPhys`, so the length check is against the count, not
        /// against the buffer size.
        fn query_phys(&self, guid: &GUID) -> Result<Vec<PhyRadioState>> {
            let mut size = 0u32;
            let mut data: *mut c_void = ptr::null_mut();
            let mut value_type = 0i32;
            let code = unsafe {
                WlanQueryInterface(
                    self.handle,
                    guid,
                    wlan_intf_opcode_radio_state,
                    ptr::null(),
                    &mut size,
                    &mut data,
                    &mut value_type,
                )
            };
            if code != 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WlanQueryInterface(radio_state) returned {code}"),
                });
            }
            if data.is_null() {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WlanQueryInterface(radio_state) returned no data".to_owned(),
                });
            }
            let guard = WlanMemory(data as *const c_void);
            let base = data.cast::<u8>();
            if size < 4 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "WlanQueryInterface(radio_state) returned {size} bytes, shorter than the PHY count"
                    ),
                });
            }
            // Safety: the count is the first four bytes of the returned buffer.
            let count = unsafe { ptr::read_unaligned(base.cast::<u32>()) };
            if count > MAX_PHYS {
                return Err(LctrlError::Unsupported {
                    feature: format!(
                        "WLAN radio state reports {count} physical layers, above the {MAX_PHYS} bound"
                    ),
                });
            }
            let required = 4 + count * 12;
            if size < required {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "WLAN radio state returned {size} bytes but reports {count} physical layers"
                    ),
                });
            }
            let mut phys = Vec::with_capacity(count as usize);
            for index in 0..count {
                // Safety: `4 + index * 12 + 12 <= size` was just checked.
                let entry = unsafe { base.add(4 + (index as usize) * 12) };
                let phy_index = unsafe { ptr::read_unaligned(entry.cast::<u32>()) };
                let software = radio_switch(unsafe {
                    ptr::read_unaligned(entry.add(4).cast::<i32>())
                });
                let hardware = radio_switch(unsafe {
                    ptr::read_unaligned(entry.add(8).cast::<i32>())
                });
                phys.push(PhyRadioState {
                    phy_index,
                    software,
                    hardware,
                });
            }
            drop(guard);
            Ok(phys)
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            unsafe { WlanCloseHandle(self.handle, ptr::null()) };
        }
    }

    /// Frees a `WlanQueryInterface`/`WlanEnumInterfaces` buffer exactly once.
    struct WlanMemory(*const c_void);

    impl Drop for WlanMemory {
        fn drop(&mut self) {
            unsafe { WlanFreeMemory(self.0) };
        }
    }

    pub(super) fn radio_switch(value: i32) -> RadioSwitch {
        if value == dot11_radio_state_on {
            RadioSwitch::On
        } else if value == dot11_radio_state_off {
            RadioSwitch::Off
        } else if value == dot11_radio_state_unknown {
            RadioSwitch::Unknown
        } else {
            // An unnamed switch value is reported as unknown; no state is invented.
            RadioSwitch::Unknown
        }
    }

    pub(super) fn link_state(value: i32) -> WlanLinkState {
        match value {
            0 => WlanLinkState::NotReady,
            1 => WlanLinkState::Connected,
            2 => WlanLinkState::AdHocNetworkFormed,
            3 => WlanLinkState::Disconnecting,
            4 => WlanLinkState::Disconnected,
            5 => WlanLinkState::Associating,
            6 => WlanLinkState::Discovering,
            7 => WlanLinkState::Authenticating,
            _ => WlanLinkState::Unknown,
        }
    }

    /// The documented per-PHY soft/hard switch state of every WLAN interface.
    pub(super) fn interfaces() -> Result<Vec<WlanRadioDetail>> {
        let client = Client::open()?;
        let interfaces = client.interfaces()?;
        Ok(interfaces
            .into_iter()
            .map(|interface| WlanRadioDetail {
                interface_guid: format!(
                    "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
                    interface.guid.data1,
                    interface.guid.data2,
                    interface.guid.data3,
                    interface.guid.data4[0],
                    interface.guid.data4[1],
                    interface
                        .guid
                        .data4
                        .iter()
                        .skip(2)
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>(),
                ),
                interface_description: if interface.description.is_empty() {
                    None
                } else {
                    Some(interface.description)
                },
                link_state: link_state(interface.link_state_raw),
                link_state_raw: interface.link_state_raw,
                phys: interface.phys,
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Adapter facts for the session-cut guard
// ---------------------------------------------------------------------------

mod adapters {
    use super::*;

    /// Every adapter with its addresses and whether it holds a gateway.
    pub(super) fn list() -> Result<Vec<NetworkInterfaceFacts>> {
        let mut size = 15 * 1024u32;
        for _ in 0..3 {
            // `Vec<u64>` keeps the buffer aligned for the returned structs.
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let code = unsafe {
                GetAdaptersAddresses(
                    u32::from(AF_UNSPEC),
                    GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST,
                    ptr::null(),
                    buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>(),
                    &mut size,
                )
            };
            if code == ERROR_BUFFER_OVERFLOW {
                continue;
            }
            if code != 0 {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("GetAdaptersAddresses returned {code}"),
                });
            }
            return Ok(collect(buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()));
        }
        Err(LctrlError::ChannelUnavailable {
            channel: "GetAdaptersAddresses kept reporting a buffer overflow".to_owned(),
        })
    }

    fn collect(mut adapter: *const IP_ADAPTER_ADDRESSES_LH) -> Vec<NetworkInterfaceFacts> {
        let mut interfaces = Vec::new();
        let mut seen = 0usize;
        while !adapter.is_null() && seen < MAX_ADAPTERS {
            seen += 1;
            // Safety: the service returned a linked list of valid `IP_ADAPTER_ADDRESSES_LH`.
            let entry = unsafe { &*adapter };
            let description = wide_string(entry.Description);
            let kind = classify(entry.IfType, &description);
            let mut addresses = Vec::new();
            let mut unicast = entry.FirstUnicastAddress;
            while !unicast.is_null() && addresses.len() < MAX_UNICAST_ADDRESSES {
                // Safety: a linked list of valid `IP_ADAPTER_UNICAST_ADDRESS_LH`.
                let address = unsafe { &*unicast };
                if let Some(address) = sockaddr_ip(address.Address) {
                    addresses.push(address);
                }
                unicast = address.Next;
            }
            let up = entry.OperStatus == IfOperStatusUp;
            interfaces.push(NetworkInterfaceFacts {
                name: wide_string(entry.FriendlyName),
                description,
                kind,
                addresses,
                has_default_gateway: up && !entry.FirstGatewayAddress.is_null(),
            });
            adapter = entry.Next;
        }
        interfaces
    }

    /// `IF_TYPE_IEEE80211` and the WWAN types map onto their radios; a Bluetooth personal area
    /// network shares the Ethernet type, so it is identified by its description.
    pub(super) fn classify(if_type: u32, description: &str) -> InterfaceKind {
        if if_type == IF_TYPE_IEEE80211 {
            return InterfaceKind::Wifi;
        }
        if if_type == IF_TYPE_WWANPP || if_type == IF_TYPE_WWANPP2 {
            return InterfaceKind::MobileBroadband;
        }
        let lowered = description.to_ascii_lowercase();
        if lowered.contains("bluetooth") {
            return InterfaceKind::BluetoothPan;
        }
        InterfaceKind::Other
    }

    fn wide_string(value: *const u16) -> String {
        if value.is_null() {
            return String::new();
        }
        let mut length = 0usize;
        // Safety: adapter strings are NUL-terminated UTF-16 owned by the service buffer.
        unsafe {
            while *value.add(length) != 0 {
                length += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(value, length))
        }
    }

    /// Convert a `SOCKET_ADDRESS` to an IP address.
    ///
    /// `sockaddr_in` carries the IPv4 address at byte offset 4; `sockaddr_in6` carries the 16-byte
    /// IPv6 address at byte offset 8 (both after the two-byte family).
    fn sockaddr_ip(address: SOCKET_ADDRESS) -> Option<IpAddr> {
        if address.lpSockaddr.is_null() || address.iSockaddrLength < 8 {
            return None;
        }
        // Safety: the service returned a sockaddr of `iSockaddrLength` bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                address.lpSockaddr.cast::<u8>(),
                address.iSockaddrLength as usize,
            )
        };
        let family = u16::from_ne_bytes([bytes[0], bytes[1]]);
        if family == AF_INET {
            Some(IpAddr::V4(Ipv4Addr::new(
                bytes[4], bytes[5], bytes[6], bytes[7],
            )))
        } else if family == AF_INET6 && bytes.len() >= 24 {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&bytes[8..24]);
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_classification_uses_the_interface_type_and_bluetooth_description() {
        assert_eq!(adapters::classify(IF_TYPE_IEEE80211, "Intel Wi-Fi"), InterfaceKind::Wifi);
        assert_eq!(
            adapters::classify(IF_TYPE_WWANPP, "Fibocom modem"),
            InterfaceKind::MobileBroadband
        );
        assert_eq!(
            adapters::classify(IF_TYPE_WWANPP2, "Fibocom modem"),
            InterfaceKind::MobileBroadband
        );
        assert_eq!(
            adapters::classify(6, "Bluetooth Device (Personal Area Network) #2"),
            InterfaceKind::BluetoothPan
        );
        assert_eq!(adapters::classify(6, "Intel Ethernet"), InterfaceKind::Other);
    }

    #[test]
    fn unknown_dot11_switch_values_are_never_promoted_to_a_named_state() {
        assert_eq!(wlan::radio_switch(dot11_radio_state_on), RadioSwitch::On);
        assert_eq!(wlan::radio_switch(dot11_radio_state_off), RadioSwitch::Off);
        assert_eq!(wlan::radio_switch(dot11_radio_state_unknown), RadioSwitch::Unknown);
        assert_eq!(wlan::radio_switch(99), RadioSwitch::Unknown);
    }

    #[test]
    fn wlan_link_states_map_by_value_and_keep_unknown_distinct() {
        assert_eq!(wlan::link_state(1), WlanLinkState::Connected);
        assert_eq!(wlan::link_state(4), WlanLinkState::Disconnected);
        assert_eq!(wlan::link_state(42), WlanLinkState::Unknown);
    }

    #[test]
    fn denied_access_is_never_reported_as_success() {
        assert!(ensure_allowed(RadioAccess::Allowed).is_ok());
        for denied in [
            RadioAccess::DeniedByUser,
            RadioAccess::DeniedBySystem,
            RadioAccess::Unspecified,
        ] {
            let error = ensure_allowed(denied).expect_err("denied access must fail");
            assert!(error.exit_code() != 0);
            assert!(!error.to_string().is_empty());
        }
    }
}

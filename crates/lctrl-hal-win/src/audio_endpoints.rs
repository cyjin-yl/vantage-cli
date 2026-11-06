//! MMDevice endpoint inventory and endpoint master volume/mute over the
//! Windows COM API.
//!
//! Contract recovered read-only from the target; see
//! `docs/evidence/re-audio-endpoints-20261105.md`:
//!
//! - `CLSID_MMDeviceEnumerator` = `{BCDE0395-E52F-467C-8E3D-C4579291692E}`,
//!   registered (`HKLM\SOFTWARE\Classes\CLSID\…\InprocServer32`) to
//!   `MMDevApi.dll`.
//! - `IMMDeviceEnumerator` / `IMMDeviceCollection` / `IMMDevice` /
//!   `IPropertyStore` / `IAudioEndpointVolume` ids were found byte-exact in
//!   `MMDevAPI.dll` 10.0.26100.8875, so the objects this module calls are the
//!   OS components rather than a Lenovo payload.
//! - Slot order is fixed by `mmdeviceapi.h` / `endpointvolume.h` /
//!   `propsys.h`; the numbers in the vtable structs below were checked against
//!   the `windows` 0.61 metadata generated from those same headers.
//!
//! Fail-closed rules used throughout:
//!
//! - every `HRESULT` is inspected; a failure becomes `ChannelUnavailable`
//!   instead of a value being read through a slot that may not be the
//!   documented one;
//! - each object is `QueryInterface`-checked for the interface the caller
//!   expects before its other slots are used;
//! - the collection count is sanity-bounded and a property the handler does
//!   not provide is reported as absent, never as an invented name;
//! - every interface pointer is owned by [`ComPtr`] and released on drop.
//!
//! Only OS state is read here. Nothing in this module selects or changes the
//! default endpoint and nothing enables/disables an endpoint.

use std::ffi::c_void;
use std::time::Duration;

use lctrl_core::{
    ApplyMode, AudioDataFlow, AudioEndpoint, AudioEndpointState, ChangeReport, EndpointVolume,
    LctrlError, Result, percent_to_scalar, volume_percent_from_scalar,
    volume_readback_tolerance_percent,
};
use lctrl_hal::{AudioEndpointControl, EndpointSelector, poll_readback_until};

// ---------------------------------------------------------------------------
// COM ABI
// ---------------------------------------------------------------------------

type Hresult = i32;

const COINIT_MULTITHREADED: u32 = 0;
const CLSCTX_ALL: u32 = 0x17;
const STGM_READ: u32 = 0;
const E_RENDER: i32 = 0;
const E_CAPTURE: i32 = 1;
const E_CONSOLE: i32 = 0;
const DEVICE_STATE_ACTIVE: u32 = 1;
const DEVICE_STATE_ALL: u32 = 0x0F;
const VT_LPWSTR: u16 = 31;
const ENDPOINT_HARDWARE_SUPPORT_VOLUME: u32 = 1;
const ENDPOINT_HARDWARE_SUPPORT_MUTE: u32 = 2;
/// `RPC_E_CHANGED_MODE`: this thread already owns a different apartment.
const RPC_E_CHANGED_MODE: Hresult = 0x8001_0106u32 as Hresult;
/// Upper bounds that turn a mis-read slot into an error instead of a loop
/// over an arbitrary count or an unbounded scan for a string terminator.
const MAX_ENDPOINTS_PER_FLOW: u32 = 1024;
const MAX_DEVICE_ID_CHARS: usize = 1024;
const READBACK_ATTEMPTS: usize = 10;
const READBACK_DELAY_MS: u64 = 50;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PropertyKey {
    fmtid: Guid,
    pid: u32,
}

/// `PROPVARIANT`: a type tag plus a 16-byte union whose payload starts at byte
/// 8 on x64. Only `VT_LPWSTR` (a `PWSTR` in the first union word) is read.
#[repr(C)]
struct PropVariant {
    vt: u16,
    reserved: [u16; 3],
    value: [u64; 2],
}

impl PropVariant {
    const fn empty() -> Self {
        Self {
            vt: 0,
            reserved: [0; 3],
            value: [0; 2],
        }
    }
}

const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xBCDE_0395,
    data2: 0xE52F,
    data3: 0x467C,
    data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
};
const IID_IMM_DEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xA956_64D2,
    data2: 0x9614,
    data3: 0x4F35,
    data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
};
const IID_IAUDIO_ENDPOINT_VOLUME: Guid = Guid {
    data1: 0x5CDF_2C82,
    data2: 0x841E,
    data3: 0x4546,
    data4: [0x97, 0x22, 0x0C, 0xF7, 0x40, 0x78, 0x22, 0x9A],
};
/// `IMMDeviceCollection` (`{0BD7A1BE-7A1A-44DB-8397-CC5392387B5E}`).
const IID_IMM_DEVICE_COLLECTION: Guid = Guid {
    data1: 0x0BD7_A1BE,
    data2: 0x7A1A,
    data3: 0x44DB,
    data4: [0x83, 0x97, 0xCC, 0x53, 0x92, 0x38, 0x7B, 0x5E],
};
/// `IMMDevice` (`{D666063F-1587-4E43-81F1-B948E807363F}`).
const IID_IMM_DEVICE: Guid = Guid {
    data1: 0xD666_063F,
    data2: 0x1587,
    data3: 0x4E43,
    data4: [0x81, 0xF1, 0xB9, 0x48, 0xE8, 0x07, 0x36, 0x3F],
};
/// `IPropertyStore` (`{886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99}`).
const IID_IPROPERTY_STORE: Guid = Guid {
    data1: 0x886D_8EEB,
    data2: 0x8CF2,
    data3: 0x4446,
    data4: [0x8D, 0x02, 0xCD, 0xBA, 0x1D, 0xBD, 0xCF, 0x99],
};
/// `functiondiscoverykeys_devpkey.h`: fmtid `A45C254E-DF1C-4EFD-8020-67D146A850E0`.
const PKEY_DEVICE_FRIENDLY_NAME: PropertyKey = PropertyKey {
    fmtid: Guid {
        data1: 0xA45C_254E,
        data2: 0xDF1C,
        data3: 0x4EFD,
        data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0],
    },
    pid: 14,
};
const PKEY_DEVICE_DEVICE_DESC: PropertyKey = PropertyKey {
    pid: 2,
    fmtid: PKEY_DEVICE_FRIENDLY_NAME.fmtid,
};

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> Hresult;
    fn CoUninitialize();
    fn CoCreateInstance(
        clsid: *const Guid,
        outer: *mut c_void,
        clsctx: u32,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> Hresult;
    fn CoTaskMemFree(pointer: *mut c_void);
    fn PropVariantClear(value: *mut PropVariant) -> Hresult;
}

/// `IUnknown`, slots 0-2 of every interface below.
#[repr(C)]
struct UnknownVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// `IMMDeviceEnumerator`: slot 3 `EnumAudioEndpoints`, 4
/// `GetDefaultAudioEndpoint`, 5 `GetDevice`; 6/7 are the (unused)
/// endpoint-notification registration slots.
#[repr(C)]
#[allow(dead_code)]
struct MmDeviceEnumeratorVtbl {
    base: UnknownVtbl,
    enum_audio_endpoints:
        unsafe extern "system" fn(*mut c_void, i32, u32, *mut *mut c_void) -> Hresult,
    get_default_audio_endpoint:
        unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> Hresult,
    get_device: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> Hresult,
    _register_endpoint_notification: *const c_void,
    _unregister_endpoint_notification: *const c_void,
}

/// `IMMDeviceCollection`: slot 3 `GetCount`, 4 `Item`.
#[repr(C)]
#[allow(dead_code)]
struct MmDeviceCollectionVtbl {
    base: UnknownVtbl,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    item: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
}

/// `IMMDevice`: slot 3 `Activate`, 4 `OpenPropertyStore`, 5 `GetId`,
/// 6 `GetState`.
#[repr(C)]
#[allow(dead_code)]
struct MmDeviceVtbl {
    base: UnknownVtbl,
    activate: unsafe extern "system" fn(
        *mut c_void,
        *const Guid,
        u32,
        *const PropVariant,
        *mut *mut c_void,
    ) -> Hresult,
    open_property_store: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
    get_id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> Hresult,
    get_state: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
}

/// `IPropertyStore`: slot 3 `GetCount`, 4 `GetAt`, 5 `GetValue`, 6 `SetValue`,
/// 7 `Commit`.
#[repr(C)]
#[allow(dead_code)]
struct PropertyStoreVtbl {
    base: UnknownVtbl,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    get_at: unsafe extern "system" fn(*mut c_void, u32, *mut PropertyKey) -> Hresult,
    get_value: unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut PropVariant) -> Hresult,
    set_value:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *const PropVariant) -> Hresult,
    commit: unsafe extern "system" fn(*mut c_void) -> Hresult,
}

/// `IAudioEndpointVolume`: slot 5 `GetChannelCount`, 7
/// `SetMasterVolumeLevelScalar`, 9 `GetMasterVolumeLevelScalar`, 14 `SetMute`,
/// 15 `GetMute`, 16 `GetVolumeStepInfo`, 19 `QueryHardwareSupport`.
#[repr(C)]
#[allow(dead_code)]
struct EndpointVolumeVtbl {
    base: UnknownVtbl,
    _register_control_change_notify: *const c_void,
    _unregister_control_change_notify: *const c_void,
    get_channel_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    _set_master_volume_level: *const c_void,
    set_master_volume_level_scalar:
        unsafe extern "system" fn(*mut c_void, f32, *const Guid) -> Hresult,
    _get_master_volume_level: *const c_void,
    get_master_volume_level_scalar: unsafe extern "system" fn(*mut c_void, *mut f32) -> Hresult,
    _set_channel_volume_level: *const c_void,
    _set_channel_volume_level_scalar: *const c_void,
    _get_channel_volume_level: *const c_void,
    _get_channel_volume_level_scalar: *const c_void,
    set_mute: unsafe extern "system" fn(*mut c_void, i32, *const Guid) -> Hresult,
    get_mute: unsafe extern "system" fn(*mut c_void, *mut i32) -> Hresult,
    get_volume_step_info: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> Hresult,
    _volume_step_up: *const c_void,
    _volume_step_down: *const c_void,
    query_hardware_support: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    _get_volume_range: *const c_void,
}

/// Owned COM interface pointer. Released on drop, so no path leaks a reference.
struct ComPtr(*mut c_void);

impl ComPtr {
    const fn as_ptr(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: every `ComPtr` is built from a pointer a successful call
            // returned, so slot 2 is that object's `IUnknown::Release`.
            unsafe { release(self.0) };
        }
    }
}

/// Read an object's vtable pointer as `T`.
///
/// # Safety
/// `object` must be a live interface pointer whose vtable is a prefix of `T`.
unsafe fn vtbl<T>(object: *mut c_void) -> *const T {
    // SAFETY: the caller guarantees the object's first word is a vtable
    // pointer with the documented layout.
    unsafe { *(object as *const *const T) }
}

/// `IUnknown::Release` (slot 2).
///
/// # Safety
/// `object` must be a live interface pointer.
unsafe fn release(object: *mut c_void) {
    // SAFETY: slot 2 of an `IUnknown`-derived vtable is `Release`.
    let vtbl = unsafe { vtbl::<UnknownVtbl>(object) };
    unsafe { ((*vtbl).release)(object) };
}

fn channel_error(context: &str, hr: Hresult) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("MMDevice {context} failed (HRESULT 0x{:08x})", hr as u32),
    }
}

fn channel_unavailable(detail: String) -> LctrlError {
    LctrlError::ChannelUnavailable { channel: detail }
}

/// Guard balancing one successful `CoInitializeEx` call.
struct ApartmentGuard(bool);

impl Drop for ApartmentGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: paired with a `CoInitializeEx` that returned S_OK or
            // S_FALSE on this thread.
            unsafe { CoUninitialize() };
        }
    }
}

/// Run `body` inside the multithreaded apartment.
///
/// `S_FALSE` ("already initialized with this mode") still requires a matching
/// `CoUninitialize`, so both non-negative results are balanced.
/// `RPC_E_CHANGED_MODE` means this thread belongs to another apartment, where
/// MMDevice objects remain usable, so the existing apartment is kept and no
/// unmatched uninitialize is issued.
fn com_apartment<T>(body: impl FnOnce() -> Result<T>) -> Result<T> {
    // SAFETY: a null `reserved` pointer is the documented argument.
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED) };
    if hr < 0 && hr != RPC_E_CHANGED_MODE {
        return Err(channel_error("CoInitializeEx", hr));
    }
    let _guard = ApartmentGuard(hr >= 0);
    body()
}

/// Succeed only if the object answers `QueryInterface` for `iid`, which proves
/// the interface identity before any other slot is called through it.
fn require_interface(object: *mut c_void, iid: &Guid, context: &str) -> Result<()> {
    let mut identity: *mut c_void = std::ptr::null_mut();
    // SAFETY: slot 0 of an `IUnknown`-derived vtable is `QueryInterface`.
    let vtbl = unsafe { vtbl::<UnknownVtbl>(object) };
    let hr = unsafe { ((*vtbl).query_interface)(object, iid, &mut identity) };
    if hr < 0 {
        return Err(channel_error(context, hr));
    }
    if !identity.is_null() {
        // SAFETY: a successful QueryInterface handed out one reference.
        unsafe { release(identity) };
    }
    Ok(())
}

fn data_flow_code(flow: AudioDataFlow) -> i32 {
    match flow {
        AudioDataFlow::Render => E_RENDER,
        AudioDataFlow::Capture => E_CAPTURE,
    }
}

// ---------------------------------------------------------------------------
// MMDevice calls
// ---------------------------------------------------------------------------

fn device_enumerator() -> Result<ComPtr> {
    let mut pointer: *mut c_void = std::ptr::null_mut();
    // SAFETY: all pointers are valid out-parameters / constants.
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_MM_DEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMM_DEVICE_ENUMERATOR,
            &mut pointer,
        )
    };
    if hr < 0 {
        return Err(channel_error("CoCreateInstance(IMMDeviceEnumerator)", hr));
    }
    if pointer.is_null() {
        return Err(channel_unavailable(
            "MMDevice CoCreateInstance returned no IMMDeviceEnumerator".into(),
        ));
    }
    let enumerator = ComPtr(pointer);
    require_interface(
        enumerator.as_ptr(),
        &IID_IMM_DEVICE_ENUMERATOR,
        "IMMDeviceEnumerator::QueryInterface",
    )?;
    Ok(enumerator)
}

fn collection_count(collection: *mut c_void) -> Result<u32> {
    let mut count = 0u32;
    // SAFETY: `collection` is a live IMMDeviceCollection.
    let vtbl = unsafe { vtbl::<MmDeviceCollectionVtbl>(collection) };
    let hr = unsafe { ((*vtbl).get_count)(collection, &mut count) };
    if hr < 0 {
        return Err(channel_error("IMMDeviceCollection::GetCount", hr));
    }
    if count > MAX_ENDPOINTS_PER_FLOW {
        return Err(channel_unavailable(format!(
            "MMDevice enumeration reported {count} endpoints, above the {MAX_ENDPOINTS_PER_FLOW} sanity bound"
        )));
    }
    Ok(count)
}

fn collection_item(collection: *mut c_void, index: u32) -> Result<ComPtr> {
    let mut device: *mut c_void = std::ptr::null_mut();
    // SAFETY: `collection` is live; `index` is below the reported count.
    let vtbl = unsafe { vtbl::<MmDeviceCollectionVtbl>(collection) };
    let hr = unsafe { ((*vtbl).item)(collection, index, &mut device) };
    if hr < 0 {
        return Err(channel_error("IMMDeviceCollection::Item", hr));
    }
    if device.is_null() {
        return Err(channel_unavailable(format!(
            "MMDevice IMMDeviceCollection::Item({index}) returned no device"
        )));
    }
    let device = ComPtr(device);
    require_interface(device.as_ptr(), &IID_IMM_DEVICE, "IMMDevice::QueryInterface")?;
    Ok(device)
}

fn enum_endpoints(
    enumerator: *mut c_void,
    flow: AudioDataFlow,
    include_inactive: bool,
) -> Result<ComPtr> {
    let state_mask = if include_inactive {
        DEVICE_STATE_ALL
    } else {
        DEVICE_STATE_ACTIVE
    };
    let mut collection: *mut c_void = std::ptr::null_mut();
    // SAFETY: `enumerator` is live; the out-pointer is valid.
    let vtbl = unsafe { vtbl::<MmDeviceEnumeratorVtbl>(enumerator) };
    let hr = unsafe {
        ((*vtbl).enum_audio_endpoints)(enumerator, data_flow_code(flow), state_mask, &mut collection)
    };
    if hr < 0 {
        return Err(channel_error("IMMDeviceEnumerator::EnumAudioEndpoints", hr));
    }
    if collection.is_null() {
        return Err(channel_unavailable(format!(
            "MMDevice EnumAudioEndpoints returned no collection for {flow}"
        )));
    }
    let collection = ComPtr(collection);
    require_interface(
        collection.as_ptr(),
        &IID_IMM_DEVICE_COLLECTION,
        "IMMDeviceCollection::QueryInterface",
    )?;
    Ok(collection)
}

fn default_endpoint(enumerator: *mut c_void, flow: AudioDataFlow) -> Result<ComPtr> {
    let mut device: *mut c_void = std::ptr::null_mut();
    // SAFETY: `enumerator` is live; the out-pointer is valid.
    let vtbl = unsafe { vtbl::<MmDeviceEnumeratorVtbl>(enumerator) };
    let hr = unsafe {
        ((*vtbl).get_default_audio_endpoint)(
            enumerator,
            data_flow_code(flow),
            E_CONSOLE,
            &mut device,
        )
    };
    if hr < 0 {
        return Err(channel_unavailable(format!(
            "no default {flow} endpoint (MMDevice GetDefaultAudioEndpoint failed, HRESULT 0x{:08x})",
            hr as u32
        )));
    }
    if device.is_null() {
        return Err(channel_unavailable(format!(
            "MMDevice GetDefaultAudioEndpoint returned no {flow} device"
        )));
    }
    let device = ComPtr(device);
    require_interface(device.as_ptr(), &IID_IMM_DEVICE, "IMMDevice::QueryInterface")?;
    Ok(device)
}

/// Endpoint id of the flow's default, or `None` when the flow has no default
/// device (a machine without a render or capture endpoint).
fn default_endpoint_id(enumerator: *mut c_void, flow: AudioDataFlow) -> Option<String> {
    let device = default_endpoint(enumerator, flow).ok()?;
    endpoint_id(device.as_ptr()).ok()
}

fn get_device(enumerator: *mut c_void, id: &str) -> Result<ComPtr> {
    let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
    let mut device: *mut c_void = std::ptr::null_mut();
    // SAFETY: `enumerator` is live, `wide` is NUL-terminated, out-pointer valid.
    let vtbl = unsafe { vtbl::<MmDeviceEnumeratorVtbl>(enumerator) };
    let hr = unsafe { ((*vtbl).get_device)(enumerator, wide.as_ptr(), &mut device) };
    if hr < 0 {
        return Err(channel_unavailable(format!(
            "no audio endpoint with id {id} (MMDevice GetDevice failed, HRESULT 0x{:08x})",
            hr as u32
        )));
    }
    if device.is_null() {
        return Err(channel_unavailable(format!(
            "MMDevice GetDevice returned no device for id {id}"
        )));
    }
    let device = ComPtr(device);
    require_interface(device.as_ptr(), &IID_IMM_DEVICE, "IMMDevice::QueryInterface")?;
    Ok(device)
}

/// Copy a `CoTaskMemAlloc`-owned NUL-terminated string and free it.
///
/// # Safety
/// `pointer` must be a NUL-terminated UTF-16 string allocated by COM.
unsafe fn take_co_task_string(pointer: *mut u16) -> String {
    let mut len = 0usize;
    // SAFETY: bounded scan for the terminator of a COM-allocated string.
    while len < MAX_DEVICE_ID_CHARS && unsafe { *pointer.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` was just measured inside the same allocation.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, len) });
    // SAFETY: the caller transfers ownership of a COM task-allocated string.
    unsafe { CoTaskMemFree(pointer.cast()) };
    text
}

fn endpoint_id(device: *mut c_void) -> Result<String> {
    let mut pointer: *mut u16 = std::ptr::null_mut();
    // SAFETY: `device` is a live IMMDevice; out-pointer is valid.
    let vtbl = unsafe { vtbl::<MmDeviceVtbl>(device) };
    let hr = unsafe { ((*vtbl).get_id)(device, &mut pointer) };
    if hr < 0 {
        return Err(channel_error("IMMDevice::GetId", hr));
    }
    if pointer.is_null() {
        return Err(channel_unavailable(
            "MMDevice IMMDevice::GetId returned no string".into(),
        ));
    }
    // SAFETY: a successful GetId returns a COM-allocated string.
    Ok(unsafe { take_co_task_string(pointer) })
}

fn endpoint_state(device: *mut c_void) -> Result<u32> {
    let mut state = 0u32;
    // SAFETY: `device` is live; out-pointer is valid.
    let vtbl = unsafe { vtbl::<MmDeviceVtbl>(device) };
    let hr = unsafe { ((*vtbl).get_state)(device, &mut state) };
    if hr < 0 {
        return Err(channel_error("IMMDevice::GetState", hr));
    }
    Ok(state)
}

fn open_property_store(device: *mut c_void) -> Result<ComPtr> {
    let mut store: *mut c_void = std::ptr::null_mut();
    // SAFETY: `device` is live; out-pointer is valid.
    let vtbl = unsafe { vtbl::<MmDeviceVtbl>(device) };
    let hr = unsafe { ((*vtbl).open_property_store)(device, STGM_READ, &mut store) };
    if hr < 0 {
        return Err(channel_error("IMMDevice::OpenPropertyStore", hr));
    }
    if store.is_null() {
        return Err(channel_unavailable(
            "MMDevice OpenPropertyStore returned no IPropertyStore".into(),
        ));
    }
    let store = ComPtr(store);
    require_interface(
        store.as_ptr(),
        &IID_IPROPERTY_STORE,
        "IPropertyStore::QueryInterface",
    )?;
    Ok(store)
}

/// Read one string property, or `None` when the handler does not provide it.
/// A missing property is absent, not fatal: the endpoint row still reports its
/// id and state.
fn property_string(store: *mut c_void, key: &PropertyKey) -> Option<String> {
    let mut value = PropVariant::empty();
    // SAFETY: `store` is live and `value` is a zeroed PROPVARIANT.
    let vtbl = unsafe { vtbl::<PropertyStoreVtbl>(store) };
    let hr = unsafe { ((*vtbl).get_value)(store, key, &mut value) };
    if hr < 0 {
        return None;
    }
    // SAFETY: a successful GetValue initializes the PROPVARIANT.
    let text = unsafe { prop_variant_string(&value) };
    // SAFETY: the PROPVARIANT holds one reference/string we must release.
    unsafe { PropVariantClear(&mut value) };
    text
}

/// # Safety
/// `value` must be an initialized `PROPVARIANT`.
unsafe fn prop_variant_string(value: &PropVariant) -> Option<String> {
    if value.vt != VT_LPWSTR {
        return None;
    }
    let pointer = value.value[0] as usize as *mut u16;
    if pointer.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: bounded scan of a string the property handler allocated.
    while len < MAX_DEVICE_ID_CHARS && unsafe { *pointer.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` was measured inside the same allocation.
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(pointer, len)
    }))
}

/// `PKEY_Device_FriendlyName`, falling back to `PKEY_Device_DeviceDesc`.
///
/// An endpoint whose property handler provides neither (or whose property
/// store will not open) gets an empty name: the name is absent, and an absent
/// name never fails the inventory or the volume commands, which key off the
/// endpoint id.
fn friendly_name(device: *mut c_void) -> String {
    let Ok(store) = open_property_store(device) else {
        return String::new();
    };
    for key in [PKEY_DEVICE_FRIENDLY_NAME, PKEY_DEVICE_DEVICE_DESC] {
        if let Some(name) = property_string(store.as_ptr(), &key) {
            if !name.is_empty() {
                return name;
            }
        }
    }
    String::new()
}

fn activate_volume(device: *mut c_void) -> Result<ComPtr> {
    let mut volume: *mut c_void = std::ptr::null_mut();
    // SAFETY: `device` is live; a null activation-property pointer is documented.
    let vtbl = unsafe { vtbl::<MmDeviceVtbl>(device) };
    let hr = unsafe {
        ((*vtbl).activate)(
            device,
            &IID_IAUDIO_ENDPOINT_VOLUME,
            CLSCTX_ALL,
            std::ptr::null(),
            &mut volume,
        )
    };
    if hr < 0 {
        return Err(channel_error("IMMDevice::Activate(IAudioEndpointVolume)", hr));
    }
    if volume.is_null() {
        return Err(channel_unavailable(
            "MMDevice Activate returned no IAudioEndpointVolume".into(),
        ));
    }
    let volume = ComPtr(volume);
    require_interface(
        volume.as_ptr(),
        &IID_IAUDIO_ENDPOINT_VOLUME,
        "IAudioEndpointVolume::QueryInterface",
    )?;
    Ok(volume)
}

/// One endpoint-volume reading.
struct VolumeReading {
    percent: u8,
    mute: bool,
    steps: u32,
    channels: u32,
    hardware: u32,
}

/// Read channel count, master volume, mute, step count and hardware support.
///
/// `None` when the endpoint volume interface cannot be activated or one of the
/// three authoritative fields fails: an inactive endpoint has no answer, and
/// no answer must not become `0 %`. The step count and hardware-support mask
/// only refine tolerance and attribution, so a failure there leaves `steps = 0`
/// (one-percent readback tolerance) and both hardware flags false.
fn volume_reading(device: *mut c_void) -> Option<VolumeReading> {
    let volume = activate_volume(device).ok()?;
    let pointer = volume.as_ptr();
    // SAFETY: `pointer` is a live IAudioEndpointVolume.
    let vtbl = unsafe { vtbl::<EndpointVolumeVtbl>(pointer) };
    let mut channels = 0u32;
    // SAFETY: out-pointer is valid for slot 5.
    if unsafe { ((*vtbl).get_channel_count)(pointer, &mut channels) } < 0 {
        return None;
    }
    let mut scalar = 0f32;
    // SAFETY: out-pointer is valid for slot 9.
    if unsafe { ((*vtbl).get_master_volume_level_scalar)(pointer, &mut scalar) } < 0 {
        return None;
    }
    let percent = volume_percent_from_scalar(scalar)?;
    let mut mute = 0i32;
    // SAFETY: out-pointer is valid for slot 15.
    if unsafe { ((*vtbl).get_mute)(pointer, &mut mute) } < 0 {
        return None;
    }
    let mut step = 0u32;
    let mut steps = 0u32;
    // SAFETY: out-pointers are valid for slot 16.
    let _ = unsafe { ((*vtbl).get_volume_step_info)(pointer, &mut step, &mut steps) };
    let mut hardware = 0u32;
    // SAFETY: out-pointer is valid for slot 19.
    let _ = unsafe { ((*vtbl).query_hardware_support)(pointer, &mut hardware) };
    Some(VolumeReading {
        percent,
        mute: mute != 0,
        steps,
        channels,
        hardware,
    })
}

/// The endpoint id, its friendly name, and the owned device.
struct ResolvedDevice {
    device: ComPtr,
    id: String,
    friendly_name: String,
}

fn resolve_device(device_id: EndpointSelector<'_>) -> Result<ResolvedDevice> {
    let enumerator = device_enumerator()?;
    let device = match device_id {
        Some(id) => get_device(enumerator.as_ptr(), id)?,
        None => default_endpoint(enumerator.as_ptr(), AudioDataFlow::Render)?,
    };
    let id = endpoint_id(device.as_ptr())?;
    let friendly_name = friendly_name(device.as_ptr());
    Ok(ResolvedDevice {
        device,
        id,
        friendly_name,
    })
}

// ---------------------------------------------------------------------------
// Sealed seam + HAL implementation
// ---------------------------------------------------------------------------

/// Sealed seam over the MMDevice surface so [`WindowsAudioEndpoints`] can be
/// exercised without a live audio stack.
pub trait MmDeviceApi: Send + Sync {
    fn endpoints(&self, include_inactive: bool) -> Result<Vec<AudioEndpoint>>;
    fn endpoint_volume(&self, device_id: EndpointSelector<'_>) -> Result<EndpointVolume>;
    fn set_master_volume(&self, device_id: EndpointSelector<'_>, percent: u8) -> Result<()>;
    fn set_mute(&self, device_id: EndpointSelector<'_>, mute: bool) -> Result<()>;
}

/// The real transport: OS COM through `MMDevAPI.dll`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMmDeviceApi;

impl MmDeviceApi for NativeMmDeviceApi {
    fn endpoints(&self, include_inactive: bool) -> Result<Vec<AudioEndpoint>> {
        com_apartment(|| enumerate_endpoints(include_inactive))
    }

    fn endpoint_volume(&self, device_id: EndpointSelector<'_>) -> Result<EndpointVolume> {
        com_apartment(|| {
            let resolved = resolve_device(device_id)?;
            let reading = volume_reading(resolved.device.as_ptr()).ok_or_else(|| {
                channel_unavailable(format!(
                    "IAudioEndpointVolume is unavailable for endpoint {}",
                    resolved.id
                ))
            })?;
            Ok(EndpointVolume {
                device_id: resolved.id,
                friendly_name: resolved.friendly_name,
                percent: reading.percent,
                mute: reading.mute,
                steps: reading.steps,
                channels: reading.channels,
                hardware_volume: reading.hardware & ENDPOINT_HARDWARE_SUPPORT_VOLUME != 0,
                hardware_mute: reading.hardware & ENDPOINT_HARDWARE_SUPPORT_MUTE != 0,
            })
        })
    }

    fn set_master_volume(&self, device_id: EndpointSelector<'_>, percent: u8) -> Result<()> {
        let scalar = percent_to_scalar(percent).ok_or_else(|| LctrlError::InvalidArgument {
            detail: format!("volume percent {percent} is outside 0..=100"),
        })?;
        com_apartment(|| {
            let resolved = resolve_device(device_id)?;
            let volume = activate_volume(resolved.device.as_ptr())?;
            // SAFETY: `volume` is a live IAudioEndpointVolume; a null event
            // context is documented as "no event".
            let vtbl = unsafe { vtbl::<EndpointVolumeVtbl>(volume.as_ptr()) };
            let hr = unsafe {
                ((*vtbl).set_master_volume_level_scalar)(volume.as_ptr(), scalar, std::ptr::null())
            };
            if hr < 0 {
                return Err(channel_error("IAudioEndpointVolume::SetMasterVolumeLevelScalar", hr));
            }
            Ok(())
        })
    }

    fn set_mute(&self, device_id: EndpointSelector<'_>, mute: bool) -> Result<()> {
        com_apartment(|| {
            let resolved = resolve_device(device_id)?;
            let volume = activate_volume(resolved.device.as_ptr())?;
            // SAFETY: `volume` is live; BOOL is a 32-bit int, context is null.
            let vtbl = unsafe { vtbl::<EndpointVolumeVtbl>(volume.as_ptr()) };
            let hr = unsafe {
                ((*vtbl).set_mute)(volume.as_ptr(), i32::from(mute), std::ptr::null())
            };
            if hr < 0 {
                return Err(channel_error("IAudioEndpointVolume::SetMute", hr));
            }
            Ok(())
        })
    }
}

fn enumerate_endpoints(include_inactive: bool) -> Result<Vec<AudioEndpoint>> {
    let enumerator = device_enumerator()?;
    let mut endpoints = Vec::new();
    for flow in [AudioDataFlow::Render, AudioDataFlow::Capture] {
        let default_id = default_endpoint_id(enumerator.as_ptr(), flow);
        let collection = enum_endpoints(enumerator.as_ptr(), flow, include_inactive)?;
        let count = collection_count(collection.as_ptr())?;
        for index in 0..count {
            let device = collection_item(collection.as_ptr(), index)?;
            let id = endpoint_id(device.as_ptr())?;
            let state = AudioEndpointState::from_raw(endpoint_state(device.as_ptr())?);
            let friendly_name = friendly_name(device.as_ptr());
            let reading = volume_reading(device.as_ptr());
            endpoints.push(AudioEndpoint {
                default: default_id.as_deref() == Some(id.as_str()),
                id,
                friendly_name,
                flow,
                state,
                channels: reading.as_ref().map(|reading| reading.channels),
                volume_percent: reading.as_ref().map(|reading| reading.percent),
                mute: reading.as_ref().map(|reading| reading.mute),
            });
        }
    }
    Ok(endpoints)
}

/// `AudioEndpointControl` over a [`MmDeviceApi`] transport.
///
/// The wrapper owns the write protocol: dry-run never reaches the transport's
/// set methods, a commit polls the readback within the endpoint's tolerance,
/// and a mismatch puts the captured value back before the error is returned.
pub struct WindowsAudioEndpoints<A: MmDeviceApi> {
    api: A,
}

impl<A: MmDeviceApi> WindowsAudioEndpoints<A> {
    pub const fn new(api: A) -> Self {
        Self { api }
    }

    pub const fn api(&self) -> &A {
        &self.api
    }

    fn readback(
        &self,
        expected: &EndpointVolume,
        device_id: EndpointSelector<'_>,
        matches: impl Fn(&EndpointVolume) -> bool,
    ) -> Result<EndpointVolume> {
        poll_readback_until(
            expected,
            READBACK_ATTEMPTS,
            Duration::from_millis(READBACK_DELAY_MS),
            || self.api.endpoint_volume(device_id),
            matches,
        )
    }

    /// Restore `previous` after a failed readback and report both outcomes.
    fn restore_after_mismatch(
        &self,
        device_id: EndpointSelector<'_>,
        previous: &EndpointVolume,
        tolerance: u8,
        error: LctrlError,
    ) -> LctrlError {
        let restored = self
            .api
            .set_master_volume(device_id, previous.percent)
            .and_then(|()| {
                self.readback(previous, device_id, |observed| {
                    observed.percent.abs_diff(previous.percent) <= tolerance
                })
            });
        match restored {
            Ok(_) => error,
            Err(restore_error) => LctrlError::FirmwareRejected {
                detail: format!(
                    "{error}; putting the previous volume back also failed: {restore_error}"
                ),
            },
        }
    }

    /// Restore the previous mute flag after a failed readback.
    fn restore_mute_after_mismatch(
        &self,
        device_id: EndpointSelector<'_>,
        previous: &EndpointVolume,
        error: LctrlError,
    ) -> LctrlError {
        let restored = self.api.set_mute(device_id, previous.mute).and_then(|()| {
            self.readback(previous, device_id, |observed| {
                observed.mute == previous.mute
            })
        });
        match restored {
            Ok(_) => error,
            Err(restore_error) => LctrlError::FirmwareRejected {
                detail: format!(
                    "{error}; putting the previous mute state back also failed: {restore_error}"
                ),
            },
        }
    }
}

impl<A: MmDeviceApi> AudioEndpointControl for WindowsAudioEndpoints<A> {
    fn endpoints(&self, include_inactive: bool) -> Result<Vec<AudioEndpoint>> {
        self.api.endpoints(include_inactive)
    }

    fn endpoint_volume(&self, device_id: EndpointSelector<'_>) -> Result<EndpointVolume> {
        self.api.endpoint_volume(device_id)
    }

    fn set_endpoint_volume(
        &self,
        device_id: EndpointSelector<'_>,
        percent: u8,
        apply: ApplyMode,
    ) -> Result<ChangeReport<EndpointVolume>> {
        if percent_to_scalar(percent).is_none() {
            return Err(LctrlError::InvalidArgument {
                detail: format!("volume percent {percent} is outside 0..=100"),
            });
        }
        let previous = self.api.endpoint_volume(device_id)?;
        let mut requested = previous.clone();
        requested.percent = percent;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let tolerance = volume_readback_tolerance_percent(previous.steps);
        self.api.set_master_volume(device_id, percent)?;
        match self.readback(&requested, device_id, |observed| {
            observed.percent.abs_diff(percent) <= tolerance
        }) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => Err(self.restore_after_mismatch(device_id, &previous, tolerance, error)),
        }
    }

    fn set_endpoint_mute(
        &self,
        device_id: EndpointSelector<'_>,
        mute: bool,
        apply: ApplyMode,
    ) -> Result<ChangeReport<EndpointVolume>> {
        let previous = self.api.endpoint_volume(device_id)?;
        let mut requested = previous.clone();
        requested.mute = mute;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        self.api.set_mute(device_id, mute)?;
        match self.readback(&requested, device_id, |observed| observed.mute == mute) {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => Err(self.restore_mute_after_mismatch(device_id, &previous, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An endpoint volume transport whose hardware curve quantizes a written
    /// percent, so the readback can legitimately differ from the request.
    struct FakeMmDevice {
        state: Mutex<EndpointVolume>,
        volume_writes: Mutex<Vec<u8>>,
        mute_writes: Mutex<Vec<bool>>,
        /// Applied to every written percent; `None` stores the request as-is.
        quantization: Option<u8>,
    }

    impl FakeMmDevice {
        fn new(percent: u8, mute: bool, steps: u32) -> Self {
            Self {
                state: Mutex::new(EndpointVolume {
                    device_id: "{0.0.0.00000000}.{2a8e3cb6-b4cf-4f88-877d-0931eec0f848}".into(),
                    friendly_name: "Speakers".into(),
                    percent,
                    mute,
                    steps,
                    channels: 2,
                    hardware_volume: true,
                    hardware_mute: true,
                }),
                volume_writes: Mutex::new(Vec::new()),
                mute_writes: Mutex::new(Vec::new()),
                quantization: None,
            }
        }

        fn quantized(percent: u8, mute: bool, steps: u32, quantum: u8) -> Self {
            Self {
                quantization: Some(quantum),
                ..Self::new(percent, mute, steps)
            }
        }
    }

    impl MmDeviceApi for FakeMmDevice {
        fn endpoints(&self, _include_inactive: bool) -> Result<Vec<AudioEndpoint>> {
            Ok(Vec::new())
        }

        fn endpoint_volume(&self, _device_id: EndpointSelector<'_>) -> Result<EndpointVolume> {
            Ok(self.state.lock().unwrap().clone())
        }

        fn set_master_volume(
            &self,
            _device_id: EndpointSelector<'_>,
            percent: u8,
        ) -> Result<()> {
            self.volume_writes.lock().unwrap().push(percent);
            let stored = match self.quantization {
                Some(quantum) => percent / quantum * quantum,
                None => percent,
            };
            self.state.lock().unwrap().percent = stored;
            Ok(())
        }

        fn set_mute(&self, _device_id: EndpointSelector<'_>, mute: bool) -> Result<()> {
            self.mute_writes.lock().unwrap().push(mute);
            self.state.lock().unwrap().mute = mute;
            Ok(())
        }
    }

    #[test]
    fn dry_run_reads_without_touching_the_transport() {
        let api = FakeMmDevice::new(42, false, 50);
        let audio = WindowsAudioEndpoints::new(api);
        let report = audio
            .set_endpoint_volume(None, 10, ApplyMode::DryRun)
            .expect("dry run");
        assert_eq!(report.mode(), ApplyMode::DryRun);
        assert_eq!(report.previous().percent, 42);
        assert_eq!(report.requested().percent, 10);
        assert!(report.actual().is_none());
        assert!(audio.api().volume_writes.lock().unwrap().is_empty());

        let report = audio
            .set_endpoint_mute(None, true, ApplyMode::DryRun)
            .expect("dry run");
        assert_eq!(report.mode(), ApplyMode::DryRun);
        assert!(!report.previous().mute);
        assert!(report.requested().mute);
        assert!(report.actual().is_none());
        assert!(audio.api().mute_writes.lock().unwrap().is_empty());
    }

    #[test]
    fn commit_accepts_a_quantized_readback_within_one_step() {
        // 50 steps = one 2-percent step; a request of 43 lands on 42.
        let api = FakeMmDevice::quantized(50, false, 50, 2);
        let audio = WindowsAudioEndpoints::new(api);
        let report = audio
            .set_endpoint_volume(None, 43, ApplyMode::Commit)
            .expect("commit");
        assert_eq!(report.mode(), ApplyMode::Commit);
        assert_eq!(report.requested().percent, 43);
        assert_eq!(report.actual().map(|actual| actual.percent), Some(42));
        assert_eq!(&*audio.api().volume_writes.lock().unwrap(), &[43]);
    }

    #[test]
    fn a_readback_outside_the_tolerance_restores_the_previous_volume() {
        // A 32-percent quantum stores 43 as 32: far outside the 2-percent
        // tolerance for a 50-step endpoint, while the captured 32 still
        // reads back exactly, so the restore itself must succeed.
        let api = FakeMmDevice::quantized(32, false, 50, 32);
        let audio = WindowsAudioEndpoints::new(api);
        let error = audio
            .set_endpoint_volume(None, 43, ApplyMode::Commit)
            .expect_err("mismatch");
        assert!(matches!(error, LctrlError::VerifyMismatch { .. }));
        assert_eq!(&*audio.api().volume_writes.lock().unwrap(), &[43, 32]);
        assert_eq!(audio.api().state.lock().unwrap().percent, 32);
    }

    #[test]
    fn mute_commit_reads_the_flag_back() {
        let api = FakeMmDevice::new(42, false, 50);
        let audio = WindowsAudioEndpoints::new(api);
        let report = audio
            .set_endpoint_mute(None, true, ApplyMode::Commit)
            .expect("commit");
        assert_eq!(report.actual().map(|actual| actual.mute), Some(true));
        assert_eq!(&*audio.api().mute_writes.lock().unwrap(), &[true]);
    }

    #[test]
    fn an_out_of_domain_percent_is_rejected_before_the_transport() {
        let api = FakeMmDevice::new(42, false, 50);
        let audio = WindowsAudioEndpoints::new(api);
        let error = audio
            .set_endpoint_volume(None, 101, ApplyMode::Commit)
            .expect_err("invalid");
        assert!(matches!(error, LctrlError::InvalidArgument { .. }));
        assert!(audio.api().volume_writes.lock().unwrap().is_empty());
    }
}

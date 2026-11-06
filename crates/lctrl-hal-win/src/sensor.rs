//! Operating-system sensors through the Windows Sensor API (`SensorsApi.dll`).
//!
//! The recovered contract (owning binary, interface table RVAs, the IDL
//! cross-check, the live 9-sensor inventory of the reference target, and the
//! HRESULT/VARTYPE observed for every call made here) is recorded in
//! `docs/evidence/re-sensors-20261105.md`. Nothing here is guessed from an
//! interface name:
//!
//! * each vtable layout below is declared in the IDL's method order and every
//!   slot that is actually called was exercised live before being wired up,
//! * every interface handed back by `CoCreateInstance`, `GetSensorsByCategory`,
//!   `ISensorCollection::GetAt` and `ISensor::GetData` is re-queried for the IID
//!   that `SensorsApi.dll` itself publishes, so a wrong slot fails closed with
//!   `E_NOINTERFACE` instead of reading a neighbouring method that happens to
//!   return `S_OK`. The one interface that cannot be re-queried that way is the
//!   `IPortableDeviceKeyCollection` from `GetSupportedDataFields`: its IID lives
//!   in `portabledevicetypes`, not in the owning DLL, so that path is pinned by
//!   its two leading slots plus the live oracle (§3 field counts, §6.2 keys)
//!   rather than by an unverified constant,
//! * whether a sensor has data is decided by the report, and whether that data is
//!   current is decided by the report's timestamp: `S_OK` with a null
//!   `ISensorDataReport` (observed on two of the nine sensors) is "no data" and
//!   never a zero value, and a report whose timestamp cannot be converted is
//!   surfaced with `fresh = false` instead of being presented as current.
//!
//! The surface is read-only. No report interval, change sensitivity, threshold,
//! or event sink is written.

use std::ffi::c_void;
use std::ptr;

use lctrl_core::{
    LctrlError, Result, SensorDescriptor, SensorField, SensorFreshness, SensorGuid, SensorInventory,
    SensorReading, SensorState, SensorValue, category_name, data_field_name, format_utc_millis,
    match_sensor, sensor_type_name, unix_millis_from_filetime, unix_millis_from_utc,
};
use lctrl_hal::SensorControl;

use windows_sys::Win32::Foundation::{FILETIME, PROPERTYKEY, SYSTEMTIME};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_sys::core::GUID;

/// `CLSID_SensorManager`, registered in `HKLM\SOFTWARE\Classes\CLSID` against
/// `C:\WINDOWS\System32\SensorsApi.dll`.
const CLSID_SENSOR_MANAGER: GUID = GUID::from_u128(0x77a1c827_fcd2_4689_8915_9d613cc5fa3e);
/// `IID_ISensorManager`. The header text shipped by mingw-w64 spells this
/// `bd7db67-…` (seven hex digits) and is rejected with `E_NOINTERFACE`; the
/// shipping binary's interface table at `SensorsApi.dll` `.rdata` RVA `0x56CB0`
/// carries this value and satisfies the same `CoCreateInstance`.
const IID_ISENSOR_MANAGER: GUID = GUID::from_u128(0xbd77db67_45a8_42dc_8d00_6dcf15f8377a);
/// `IID_ISensorCollection`, `SensorsApi.dll` `.rdata` RVA `0x56CA0`.
const IID_ISENSOR_COLLECTION: GUID = GUID::from_u128(0x23571e11_e545_4dd8_a337_b89bf44b10df);
/// `IID_ISensor`, `SensorsApi.dll` `.rdata` RVA `0x56CD0`.
const IID_ISENSOR: GUID = GUID::from_u128(0x5fa08f80_2657_458e_af75_46f73fa6ac5c);
/// `IID_ISensorDataReport`, `SensorsApi.dll` `.rdata` RVA `0x56D00`.
const IID_ISENSOR_DATA_REPORT: GUID = GUID::from_u128(0x0ab9df9b_c4b5_4796_8898_0470706a2e1d);
/// `SENSOR_CATEGORY_ALL`: every category the platform can enumerate.
const SENSOR_CATEGORY_ALL: GUID = GUID::from_u128(0xc317c286_c468_4288_9975_d4c4587c442c);
/// `SENSOR_PROPERTY_COMMON_GUID`: every `SENSOR_PROPERTY_*` key shares this FMTID.
const SENSOR_PROPERTY_COMMON_GUID: GUID =
    GUID::from_u128(0x7f8383ec_d3ec_495c_a8cf_b8bbe85c2920);

/// `SENSOR_PROPERTY_MANUFACTURER`.
const SENSOR_PROPERTY_MANUFACTURER: PROPERTYKEY = property_key(6);
/// `SENSOR_PROPERTY_MODEL`.
const SENSOR_PROPERTY_MODEL: PROPERTYKEY = property_key(7);
/// `SENSOR_PROPERTY_SERIAL_NUMBER`.
const SENSOR_PROPERTY_SERIAL_NUMBER: PROPERTYKEY = property_key(8);
/// `SENSOR_PROPERTY_FRIENDLY_NAME`.
const SENSOR_PROPERTY_FRIENDLY_NAME: PROPERTYKEY = property_key(9);
/// `SENSOR_PROPERTY_MIN_REPORT_INTERVAL`, in milliseconds.
const SENSOR_PROPERTY_MIN_REPORT_INTERVAL: PROPERTYKEY = property_key(12);
/// `SENSOR_PROPERTY_DEVICE_PATH`: the device-interface instance this sensor lives
/// on, which is the identity that survives re-enumeration.
const SENSOR_PROPERTY_DEVICE_PATH: PROPERTYKEY = property_key(15);

const fn property_key(pid: u32) -> PROPERTYKEY {
    PROPERTYKEY {
        fmtid: SENSOR_PROPERTY_COMMON_GUID,
        pid,
    }
}

/// `RPC_E_CHANGED_MODE`: COM is already initialized in another apartment. The
/// interface is still usable; only `CoUninitialize` must be skipped.
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

/// Sensors read through the public Sensor API. Stateless: every call takes its
/// own apartment, manager, and enumeration.
#[derive(Debug, Default)]
pub struct WindowsSensors;

impl WindowsSensors {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SensorControl for WindowsSensors {
    fn list(&self) -> Result<SensorInventory> {
        let _apartment = Apartment::enter()?;
        let manager = SensorManager::create()?;
        let collection = manager.sensors_by_category(&SENSOR_CATEGORY_ALL)?;
        let count = collection.count()?;
        let now_ms = now_unix_millis();
        let mut sensors = Vec::new();
        let mut unavailable = Vec::new();
        for index in 0..count {
            let sensor = collection.at(index)?;
            let keys = sensor.supported_data_fields()?;
            let mut descriptor = sensor.describe(index as usize, keys.len())?;
            match sensor.report(&keys) {
                Ok((report_ms, fields)) => sensors.push(SensorReading {
                    report_timestamp_utc: report_ms.map(format_utc_millis),
                    freshness: SensorFreshness::between(
                        now_ms,
                        report_ms,
                        descriptor.min_report_interval_ms,
                    ),
                    descriptor,
                    fields,
                }),
                Err(status) => {
                    descriptor.report_status = Some(status);
                    unavailable.push(descriptor);
                }
            }
        }
        Ok(SensorInventory {
            sensors,
            unavailable,
        })
    }

    fn read(&self, target: &str) -> Result<SensorReading> {
        let _apartment = Apartment::enter()?;
        let manager = SensorManager::create()?;
        let collection = manager.sensors_by_category(&SENSOR_CATEGORY_ALL)?;
        let count = collection.count()?;
        // Identity only: the selected sensor is the one that gets a report.
        let mut entries = Vec::new();
        for index in 0..count {
            let sensor = collection.at(index)?;
            let keys = sensor.supported_data_fields()?;
            let descriptor = sensor.describe(index as usize, keys.len())?;
            entries.push((sensor, keys, descriptor));
        }
        let descriptors: Vec<SensorDescriptor> = entries
            .iter()
            .map(|(_, _, descriptor)| descriptor.clone())
            .collect();
        let position = match_sensor(&descriptors, target)?;
        let (sensor, keys, descriptor) = &entries[position];
        let now_ms = now_unix_millis();
        match sensor.report(keys) {
            Ok((report_ms, fields)) => Ok(SensorReading {
                report_timestamp_utc: report_ms.map(format_utc_millis),
                freshness: SensorFreshness::between(
                    now_ms,
                    report_ms,
                    descriptor.min_report_interval_ms,
                ),
                descriptor: descriptor.clone(),
                fields,
            }),
            Err(status) => Err(LctrlError::Unsupported {
                feature: format!("sensor '{}' returned no data report ({status})", descriptor.label()),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// COM apartment
// ---------------------------------------------------------------------------

/// A single-threaded apartment for the lifetime of one call.
///
/// The sensor object graph is created and used on this thread only. `S_FALSE`
/// means the thread was already initialized in the same mode and must not be
/// uninitialized here; `RPC_E_CHANGED_MODE` means another mode won (the CLI also
/// talks to WMI and RPC), and COM is already usable in that case.
struct Apartment {
    uninitialize: bool,
}

impl Apartment {
    fn enter() -> Result<Self> {
        let result = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        match result {
            0 => Ok(Self {
                uninitialize: true,
            }),
            1 | RPC_E_CHANGED_MODE => Ok(Self {
                uninitialize: false,
            }),
            _ => Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "CoInitializeEx(APARTMENTTHREADED) failed with HRESULT 0x{:08X}",
                    result as u32
                ),
            }),
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        }
    }
}

// ---------------------------------------------------------------------------
// Sensor API ABI (hand-written, method order from the public IDL)
// ---------------------------------------------------------------------------

/// A vtable slot this module never calls. Declared so the slots it does call keep
/// their documented offset instead of being counted by hand.
type Slot = *const c_void;

#[repr(C)]
struct IUnknownVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: Slot,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// `ISensorManager`: GetSensorsByCategory, GetSensorsByType, GetSensorByID,
/// SetEventSink, RequestPermissions.
#[repr(C)]
struct SensorManagerVtbl {
    base: IUnknownVtbl,
    get_sensors_by_category:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    get_sensors_by_type: Slot,
    get_sensor_by_id: Slot,
    set_event_sink: Slot,
    request_permissions: Slot,
}

/// `ISensorCollection`: GetAt, GetCount, Add, Remove, RemoveByID, Clear.
#[repr(C)]
struct SensorCollectionVtbl {
    base: IUnknownVtbl,
    get_at: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    add: Slot,
    remove: Slot,
    remove_by_id: Slot,
    clear: Slot,
}

/// `ISensor`: GetID, GetCategory, GetType, GetFriendlyName, GetProperty,
/// GetProperties, GetSupportedDataFields, SetProperties, SupportsDataField,
/// GetState, GetData, SupportsEvent, GetEventInterest, SetEventInterest,
/// SetEventSink.
#[repr(C)]
struct SensorVtbl {
    base: IUnknownVtbl,
    // `SENSOR_ID` is a `PROPERTYKEY` on the wire; this module reports identity
    // from `SENSOR_PROPERTY_DEVICE_PATH` instead, because the reference driver
    // returns its own LUID encoding here rather than a real property key.
    get_id: Slot,
    get_category: unsafe extern "system" fn(*mut c_void, *mut GUID) -> i32,
    get_type: unsafe extern "system" fn(*mut c_void, *mut GUID) -> i32,
    get_friendly_name: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32,
    get_property: unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PropVariant) -> i32,
    get_properties: Slot,
    get_supported_data_fields: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    set_properties: Slot,
    supports_data_field: Slot,
    get_state: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    get_data: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    supports_event: Slot,
    get_event_interest: Slot,
    set_event_interest: Slot,
    set_event_sink: Slot,
}

/// `ISensorDataReport`: GetTimestamp, GetSensorValue, GetSensorValues.
#[repr(C)]
struct SensorDataReportVtbl {
    base: IUnknownVtbl,
    get_timestamp: unsafe extern "system" fn(*mut c_void, *mut SYSTEMTIME) -> i32,
    get_sensor_value:
        unsafe extern "system" fn(*mut c_void, *const PROPERTYKEY, *mut PropVariant) -> i32,
    get_sensor_values: Slot,
}

/// `IPortableDeviceKeyCollection`: GetCount, GetAt, Add, Clear, RemoveAt.
#[repr(C)]
struct KeyCollectionVtbl {
    base: IUnknownVtbl,
    get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    get_at: unsafe extern "system" fn(*mut c_void, u32, *mut PROPERTYKEY) -> i32,
    add: Slot,
    clear: Slot,
    remove_at: Slot,
}

/// Read the vtable of a live interface as `T`.
///
/// # Safety
/// `this` must be a live COM interface pointer whose vtable has layout `T`.
unsafe fn vtable<'a, T>(this: *mut c_void) -> &'a T {
    unsafe { &**(this as *const *const T) }
}

/// `PROPVARIANT`: a `VARTYPE`, three reserved words, and a 16-byte payload.
#[repr(C)]
#[derive(Clone, Copy)]
struct PropVariant {
    vartype: u16,
    reserved: [u16; 3],
    payload: [u64; 2],
}

impl PropVariant {
    const fn zeroed() -> Self {
        Self {
            vartype: 0,
            reserved: [0; 3],
            payload: [0; 2],
        }
    }

    fn bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.payload[0].to_ne_bytes());
        out[8..].copy_from_slice(&self.payload[1].to_ne_bytes());
        out
    }

    fn pointer(&self) -> *const c_void {
        self.payload[0] as usize as *const c_void
    }

    /// Decode, then release whatever the platform allocated inside the variant.
    fn take(self) -> SensorValue {
        let decoded = self.decode();
        let mut owned = self;
        unsafe { PropVariantClear(&mut owned) };
        decoded
    }

    fn decode(&self) -> SensorValue {
        match self.vartype {
            // VT_EMPTY, VT_NULL
            0 | 1 => SensorValue::Empty,
            // VT_I2
            2 => SensorValue::Integer {
                value: i64::from(self.payload[0] as u16 as i16),
            },
            // VT_I4
            3 => SensorValue::Integer {
                value: i64::from(self.payload[0] as u32 as i32),
            },
            // VT_R4
            4 => SensorValue::Real {
                value: f64::from(f32::from_bits(self.payload[0] as u32)),
            },
            // VT_R8
            5 => SensorValue::Real {
                value: f64::from_bits(self.payload[0]),
            },
            // VT_BSTR
            8 => wide_string(self.pointer().cast()).map_or(SensorValue::Empty, |value| {
                SensorValue::Text { value }
            }),
            // VT_BOOL
            11 => SensorValue::Boolean {
                value: (self.payload[0] as u16) != 0,
            },
            // VT_UI1, VT_UI2, VT_UI4, VT_UI8
            17 => SensorValue::Unsigned {
                value: u64::from(self.payload[0] as u8),
            },
            18 => SensorValue::Unsigned {
                value: u64::from(self.payload[0] as u16),
            },
            19 => SensorValue::Unsigned {
                value: u64::from(self.payload[0] as u32),
            },
            21 => SensorValue::Unsigned {
                value: self.payload[0],
            },
            // VT_I8
            20 => SensorValue::Integer {
                value: self.payload[0] as i64,
            },
            // VT_LPSTR
            30 => ansi_string(self.pointer().cast()).map_or(SensorValue::Empty, |value| {
                SensorValue::Text { value }
            }),
            // VT_LPWSTR
            31 => wide_string(self.pointer().cast()).map_or(SensorValue::Empty, |value| {
                SensorValue::Text { value }
            }),
            // VT_FILETIME
            64 => {
                let filetime = self.payload[0];
                unix_millis_from_filetime(filetime).map_or(
                    SensorValue::Empty,
                    |millis| SensorValue::Timestamp {
                        utc: format_utc_millis(millis),
                        filetime,
                    },
                )
            }
            // VT_CLSID
            72 => {
                let bytes = self.bytes();
                SensorValue::Guid {
                    value: SensorGuid::from_bytes(bytes),
                }
            }
            other => {
                let bytes = self.bytes();
                let mut raw_hex = String::with_capacity(32);
                for byte in bytes {
                    use std::fmt::Write as _;
                    let _ = write!(raw_hex, "{byte:02X}");
                }
                SensorValue::Unsupported {
                    vartype: u32::from(other),
                    raw_hex,
                }
            }
        }
    }
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn PropVariantClear(pvar: *mut PropVariant) -> i32;
}

#[link(name = "oleaut32")]
unsafe extern "system" {
    fn SysFreeString(bstr: *const u16);
}

fn wide_string(pointer: *const u16) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let length = unsafe { wide_length(pointer) };
    let slice = unsafe { std::slice::from_raw_parts(pointer, length) };
    Some(String::from_utf16_lossy(slice))
}

/// # Safety
/// `pointer` must point at a NUL-terminated UTF-16 string.
unsafe fn wide_length(pointer: *const u16) -> usize {
    let mut length = 0usize;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    length
}

fn ansi_string(pointer: *const u8) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let mut length = 0usize;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let slice = unsafe { std::slice::from_raw_parts(pointer, length) };
    Some(String::from_utf8_lossy(slice).into_owned())
}

/// An owned interface. Released exactly once on drop.
struct Interface {
    raw: *mut c_void,
}

impl Interface {
    fn adopt(raw: *mut c_void, context: &'static str) -> Result<Self> {
        if raw.is_null() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{context} returned a null interface pointer"),
            });
        }
        Ok(Self { raw })
    }

    /// Fail closed when the platform hands back an interface of another type:
    /// the pointer is re-queried for its documented IID, so a mis-declared slot
    /// reports `E_NOINTERFACE` instead of silently reading a wrong method.
    fn adopt_as(raw: *mut c_void, iid: &GUID, context: &'static str) -> Result<Self> {
        let owned = Self::adopt(raw, context)?;
        let mut requeryed: *mut c_void = ptr::null_mut();
        let result = unsafe {
            (vtable::<IUnknownVtbl>(owned.raw).query_interface)(owned.raw, iid, &mut requeryed)
        };
        if result < 0 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{context} did not expose its documented interface (QueryInterface 0x{:08X})",
                    result as u32
                ),
            });
        }
        Self::adopt(requeryed, context)
    }
}

impl Drop for Interface {
    fn drop(&mut self) {
        unsafe { (vtable::<IUnknownVtbl>(self.raw).release)(self.raw) };
    }
}

fn check(result: i32, context: &'static str) -> Result<()> {
    if result < 0 {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("{context} failed with HRESULT 0x{:08X}", result as u32),
        });
    }
    Ok(())
}

fn guid_to_sensor_guid(guid: &GUID) -> SensorGuid {
    let mut bytes = [0u8; 16];
    bytes[..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..].copy_from_slice(&guid.data4);
    SensorGuid::from_bytes(bytes)
}

fn property_key_label(key: &PROPERTYKEY) -> String {
    format!("{},{}", guid_to_sensor_guid(&key.fmtid), key.pid)
}

fn now_unix_millis() -> Option<i64> {
    let mut raw = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    unsafe { GetSystemTimeAsFileTime(&mut raw) };
    let ticks = (u64::from(raw.dwHighDateTime) << 32) | u64::from(raw.dwLowDateTime);
    unix_millis_from_filetime(ticks)
}

fn systemtime_to_unix_millis(timestamp: &SYSTEMTIME) -> Option<i64> {
    unix_millis_from_utc(
        i64::from(timestamp.wYear),
        i64::from(timestamp.wMonth),
        i64::from(timestamp.wDay),
        i64::from(timestamp.wHour),
        i64::from(timestamp.wMinute),
        i64::from(timestamp.wSecond),
        i64::from(timestamp.wMilliseconds),
    )
}

// ---------------------------------------------------------------------------
// Sensor API handles
// ---------------------------------------------------------------------------

struct SensorManager {
    interface: Interface,
}

impl SensorManager {
    fn create() -> Result<Self> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            CoCreateInstance(
                &CLSID_SENSOR_MANAGER,
                ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_ISENSOR_MANAGER,
                &mut raw,
            )
        };
        check(
            result,
            "CoCreateInstance(CLSID_SensorManager, IID_ISensorManager)",
        )?;
        Ok(Self {
            interface: Interface::adopt(raw, "CoCreateInstance(CLSID_SensorManager)")?,
        })
    }

    fn sensors_by_category(&self, category: &GUID) -> Result<SensorCollection> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            (vtable::<SensorManagerVtbl>(self.interface.raw).get_sensors_by_category)(
                self.interface.raw,
                category,
                &mut raw,
            )
        };
        check(result, "ISensorManager::GetSensorsByCategory")?;
        Ok(SensorCollection {
            interface: Interface::adopt_as(
                raw,
                &IID_ISENSOR_COLLECTION,
                "ISensorManager::GetSensorsByCategory",
            )?,
        })
    }
}

struct SensorCollection {
    interface: Interface,
}

impl SensorCollection {
    fn count(&self) -> Result<u32> {
        let mut count = 0u32;
        let result = unsafe {
            (vtable::<SensorCollectionVtbl>(self.interface.raw).get_count)(
                self.interface.raw,
                &mut count,
            )
        };
        check(result, "ISensorCollection::GetCount")?;
        Ok(count)
    }

    fn at(&self, index: u32) -> Result<Sensor> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            (vtable::<SensorCollectionVtbl>(self.interface.raw).get_at)(
                self.interface.raw,
                index,
                &mut raw,
            )
        };
        check(result, "ISensorCollection::GetAt")?;
        Ok(Sensor {
            interface: Interface::adopt_as(raw, &IID_ISENSOR, "ISensorCollection::GetAt")?,
        })
    }
}

struct Sensor {
    interface: Interface,
}

impl Sensor {
    fn category(&self) -> Result<SensorGuid> {
        let mut guid = GUID::default();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_category)(self.interface.raw, &mut guid)
        };
        check(result, "ISensor::GetCategory")?;
        Ok(guid_to_sensor_guid(&guid))
    }

    fn sensor_type(&self) -> Result<SensorGuid> {
        let mut guid = GUID::default();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_type)(self.interface.raw, &mut guid)
        };
        check(result, "ISensor::GetType")?;
        Ok(guid_to_sensor_guid(&guid))
    }

    fn friendly_name(&self) -> Option<String> {
        let mut raw: *mut u16 = ptr::null_mut();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_friendly_name)(
                self.interface.raw,
                &mut raw,
            )
        };
        if result < 0 || raw.is_null() {
            return None;
        }
        let name = wide_string(raw);
        unsafe { SysFreeString(raw) };
        name
    }

    /// One `SENSOR_PROPERTY_*` value. A property the sensor does not publish
    /// (`ERROR_NOT_FOUND`) is `None`, and any other failure is also `None`: this
    /// is the identity channel, and an identity that cannot be read is reported
    /// as absent rather than invented.
    fn property(&self, key: &PROPERTYKEY) -> Option<SensorValue> {
        let mut variant = PropVariant::zeroed();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_property)(
                self.interface.raw,
                key,
                &mut variant,
            )
        };
        if result < 0 {
            // `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)` (`0x80070490`) is the
            // documented "this sensor does not publish the property"; any other
            // failure leaves the identity field absent as well. Neither case
            // invents a value.
            return None;
        }
        Some(variant.take())
    }

    fn text_property(&self, key: &PROPERTYKEY) -> Option<String> {
        match self.property(key)? {
            SensorValue::Text { value } => Some(value),
            _ => None,
        }
    }

    fn u32_property(&self, key: &PROPERTYKEY) -> Option<u32> {
        match self.property(key)? {
            SensorValue::Unsigned { value } => u32::try_from(value).ok(),
            _ => None,
        }
    }

    fn state(&self) -> Result<SensorState> {
        let mut raw = 0i32;
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_state)(self.interface.raw, &mut raw)
        };
        check(result, "ISensor::GetState")?;
        Ok(SensorState::from_raw(raw))
    }

    /// Every data field the sensor says it publishes. Read-only: this asks the
    /// sensor for its own key list instead of assuming a layout.
    fn supported_data_fields(&self) -> Result<Vec<PROPERTYKEY>> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_supported_data_fields)(
                self.interface.raw,
                &mut raw,
            )
        };
        check(result, "ISensor::GetSupportedDataFields")?;
        if raw.is_null() {
            return Ok(Vec::new());
        }
        let collection = Interface::adopt(raw, "ISensor::GetSupportedDataFields")?;
        let mut count = 0u32;
        let result = unsafe {
            (vtable::<KeyCollectionVtbl>(collection.raw).get_count)(collection.raw, &mut count)
        };
        check(result, "IPortableDeviceKeyCollection::GetCount")?;
        let mut keys = Vec::with_capacity(count as usize);
        for index in 0..count {
            let mut key = PROPERTYKEY::default();
            let result = unsafe {
                (vtable::<KeyCollectionVtbl>(collection.raw).get_at)(collection.raw, index, &mut key)
            };
            check(result, "IPortableDeviceKeyCollection::GetAt")?;
            keys.push(key);
        }
        Ok(keys)
    }

    fn describe(&self, index: usize, data_field_count: usize) -> Result<SensorDescriptor> {
        let category = self.category()?;
        let sensor_type = self.sensor_type()?;
        Ok(SensorDescriptor {
            index,
            friendly_name: self.friendly_name(),
            manufacturer: self.text_property(&SENSOR_PROPERTY_MANUFACTURER),
            model: self.text_property(&SENSOR_PROPERTY_MODEL),
            serial_number: self.text_property(&SENSOR_PROPERTY_SERIAL_NUMBER),
            device_path: self.text_property(&SENSOR_PROPERTY_DEVICE_PATH),
            category_name: category_name(category),
            category,
            sensor_type_name: sensor_type_name(sensor_type),
            sensor_type,
            state: self.state()?,
            min_report_interval_ms: self.u32_property(&SENSOR_PROPERTY_MIN_REPORT_INTERVAL),
            data_field_count,
            report_status: None,
        })
    }

    /// One report: its timestamp in Unix milliseconds (when it converts) and
    /// every supported field with the value the platform delivered.
    ///
    /// `Ok` means the platform produced a report — an empty `fields` list is a
    /// report that carries no data field, not a failure. `Err` carries why no
    /// report could be taken, so the sensor is listed as unavailable instead of
    /// being given a zero reading.
    fn report(
        &self,
        keys: &[PROPERTYKEY],
    ) -> std::result::Result<(Option<i64>, Vec<SensorField>), String> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result = unsafe {
            (vtable::<SensorVtbl>(self.interface.raw).get_data)(self.interface.raw, &mut raw)
        };
        if result < 0 {
            return Err(format!(
                "ISensor::GetData failed with HRESULT 0x{:08X}",
                result as u32
            ));
        }
        if raw.is_null() {
            // Observed on 2 of the 9 sensors of the reference target: the call
            // succeeds and hands back no report at all.
            return Err("the platform returned S_OK without a data report".into());
        }
        let report = Interface::adopt_as(raw, &IID_ISENSOR_DATA_REPORT, "ISensor::GetData")
            .map_err(|error| error.to_string())?;
        let mut raw_timestamp = SYSTEMTIME::default();
        let timestamp = {
            let result = unsafe {
                (vtable::<SensorDataReportVtbl>(report.raw).get_timestamp)(
                    report.raw,
                    &mut raw_timestamp,
                )
            };
            if result < 0 {
                None
            } else {
                systemtime_to_unix_millis(&raw_timestamp)
            }
        };
        let mut fields = Vec::with_capacity(keys.len());
        for key in keys {
            let mut variant = PropVariant::zeroed();
            let result = unsafe {
                (vtable::<SensorDataReportVtbl>(report.raw).get_sensor_value)(
                    report.raw,
                    key,
                    &mut variant,
                )
            };
            if result < 0 {
                // The sensor advertised the field but did not publish it in this
                // report: the field is absent, not zero.
                continue;
            }
            let fmtid = guid_to_sensor_guid(&key.fmtid);
            fields.push(SensorField {
                key: property_key_label(key),
                name: data_field_name(fmtid, key.pid),
                value: variant.take(),
            });
        }
        Ok((timestamp, fields))
    }
}

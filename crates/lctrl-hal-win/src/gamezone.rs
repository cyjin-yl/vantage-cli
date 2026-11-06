//! Lenovo firmware GAMEZONE control plane (`root\wmi`).
//!
//! Reversed interface semantics (see `docs/interfaces.md` §10):
//!
//! - `LENOVO_GAMEZONE_DATA` exposes the Legion/ThinkBook game-zone feature
//!   set: thermal mode, fan cooling, smart fan, keyboard light, Win-key lock,
//!   touchpad toggle, panel overdrive, charge-mode readout, CPU/GPU
//!   temperature and frequency, and overclocking support probes. Setters take
//!   a single `Data: u32` input; `SetIGPUModeStatus` names it `mode`.
//! - `LENOVO_FAN_METHOD.Fan_Get_Table(FanID: u8, SensorID: u8)` /
//!   `Fan_Set_Table` carry the firmware fan curve.
//! - `LENOVO_OTHER_METHOD` is the generic feature plane:
//!   `GetFeatureValue(IDs: u32)`, `SetFeatureValue(IDs: u32, value: u32)`,
//!   `GetDataByCommand(Command: u32, IDs: u32)`, `GetDataByPackage(Input: u8)`.
//!
//! Firmware support is model-dependent: Legion-class machines implement these
//! providers; ThinkBook 21VG enumerates the classes but the provider rejects
//! invocation with `WBEM_E_INVALID_METHOD_PARAMETERS`. Every operation here
//! resolves the active instance per call and reports the rejection verbatim,
//! so machines without support fail as `unsupported`, never as silent success.

use lctrl_core::{LctrlError, Result};

use crate::{WmiObject, WmiTransport, WmiValue, active_instance, wmi_contract::ROOT_WMI};

const GAMEZONE_CLASS: &str = "LENOVO_GAMEZONE_DATA";
const FAN_METHOD_CLASS: &str = "LENOVO_FAN_METHOD";
const OTHER_METHOD_CLASS: &str = "LENOVO_OTHER_METHOD";

/// Firmware GAMEZONE control client. Generic over the WMI transport so unit
/// tests can bind a scripted fake.
pub struct GameZone<W> {
    wmi: W,
}

impl<W> GameZone<W> {
    #[must_use]
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

impl<W> GameZone<W>
where
    W: WmiTransport,
{
    fn invoke(&self, class: &str, method: &str, input: WmiObject) -> Result<WmiObject> {
        let instance = active_instance(&self.wmi, class)?;
        self.wmi
            .invoke_instance(ROOT_WMI, class, instance.path(), method, &input)
    }

    fn get_u32(&self, method: &str) -> Result<u32> {
        let output = self.invoke(GAMEZONE_CLASS, method, WmiObject::new())?;
        first_u32(&output, method)
    }

    fn set_data(&self, method: &str, data: u32) -> Result<()> {
        let mut input = WmiObject::new();
        input.insert("Data".into(), WmiValue::U32(data));
        let output = self.invoke(GAMEZONE_CLASS, method, input)?;
        require_return_zero(&output, method)
    }

    /// Cheap readiness probe: the current firmware thermal mode.
    pub fn thermal_mode(&self) -> Result<u32> {
        self.get_u32("GetThermalMode")
    }

    pub fn power_charge_mode(&self) -> Result<u32> {
        self.get_u32("GetPowerChargeMode")
    }

    pub fn fan_cooling_status(&self) -> Result<u32> {
        self.get_u32("GetFanCoolingStatus")
    }

    pub fn smart_fan_mode(&self) -> Result<u32> {
        self.get_u32("GetSmartFanMode")
    }

    pub fn keyboard_light(&self) -> Result<u32> {
        self.get_u32("GetKeyboardLight")
    }

    pub fn winkey_status(&self) -> Result<u32> {
        self.get_u32("GetWinKeyStatus")
    }

    pub fn touchpad_status(&self) -> Result<u32> {
        self.get_u32("GetTPStatus")
    }

    pub fn od_status(&self) -> Result<u32> {
        self.get_u32("GetODStatus")
    }

    pub fn cpu_temp(&self) -> Result<u32> {
        self.get_u32("GetCPUTemp")
    }

    pub fn gpu_temp(&self) -> Result<u32> {
        self.get_u32("GetGPUTemp")
    }

    pub fn cpu_frequency(&self) -> Result<u32> {
        self.get_u32("GetCpuFrequency")
    }

    /// Read one `Is*` support probe's raw `Data` answer.
    ///
    /// Lenovo reports support through `Data`: `1` means the feature is
    /// present, `0` means it is absent (`docs/01` §2.6, `docs/03` §5.4).
    /// `ReturnValue` only says the provider executed the method, so a `False`
    /// `ReturnValue` is a firmware rejection of the call, never "absent".
    pub fn support_data(&self, probe: SupportProbe) -> Result<u32> {
        let output = self.invoke(GAMEZONE_CLASS, probe.method(), WmiObject::new())?;
        let return_value = match output.get("ReturnValue") {
            Some(WmiValue::Bool(value)) => *value,
            Some(value) => numeric(value, probe.method())? != 0,
            None => true,
        };
        if !return_value {
            return Err(LctrlError::FirmwareRejected {
                detail: format!(
                    "{GAMEZONE_CLASS}.{} answered ReturnValue=False",
                    probe.method()
                ),
            });
        }
        first_u32(&output, probe.method())
    }

    /// `Is*` support probe: `Data = 1` means supported, `0` means absent.
    pub fn is_supported(&self, probe: SupportProbe) -> Result<bool> {
        Ok(self.support_data(probe)? == 1)
    }

    pub fn set_fan_cooling(&self, data: u32) -> Result<()> {
        self.set_data("SetFanCooling", data)
    }

    pub fn set_smart_fan_mode(&self, data: u32) -> Result<()> {
        self.set_data("SetSmartFanMode", data)
    }

    pub fn set_keyboard_light(&self, data: u32) -> Result<()> {
        self.set_data("SetKeyboardLight", data)
    }

    pub fn set_winkey_status(&self, data: u32) -> Result<()> {
        self.set_data("SetWinKeyStatus", data)
    }

    pub fn set_touchpad_status(&self, data: u32) -> Result<()> {
        self.set_data("SetTPStatus", data)
    }

    pub fn set_od_status(&self, data: u32) -> Result<()> {
        self.set_data("SetODStatus", data)
    }

    /// Read the firmware fan curve for one fan/sensor pair.
    pub fn fan_get_table(&self, fan_id: u8, sensor_id: u8) -> Result<WmiObject> {
        let mut input = WmiObject::new();
        input.insert("FanID".into(), WmiValue::U8(fan_id));
        input.insert("SensorID".into(), WmiValue::U8(sensor_id));
        self.invoke(FAN_METHOD_CLASS, "Fan_Get_Table", input)
    }

    /// Generic feature-plane read. `ids` is the firmware feature identifier
    /// (reversed constants live at the call site; this layer does not invent
    /// identifiers).
    pub fn feature_value(&self, ids: FeatureId) -> Result<u32> {
        let mut input = WmiObject::new();
        input.insert("IDs".into(), WmiValue::U32(ids.raw()));
        let output = self.invoke(OTHER_METHOD_CLASS, "GetFeatureValue", input)?;
        first_u32(&output, "GetFeatureValue")
    }

    /// Generic feature-plane write, used by firmware without dedicated setters
    /// (e.g. thermal mode / charge mode on this interface).
    pub fn set_feature_value(&self, ids: FeatureId, value: u32) -> Result<()> {
        let mut input = WmiObject::new();
        input.insert("IDs".into(), WmiValue::U32(ids.raw()));
        input.insert("value".into(), WmiValue::U32(value));
        let output = self.invoke(OTHER_METHOD_CLASS, "SetFeatureValue", input)?;
        require_return_zero(&output, "SetFeatureValue")
    }

    pub fn data_by_command(&self, command: u32, ids: FeatureId) -> Result<WmiObject> {
        let mut input = WmiObject::new();
        input.insert("Command".into(), WmiValue::U32(command));
        input.insert("IDs".into(), WmiValue::U32(ids.raw()));
        self.invoke(OTHER_METHOD_CLASS, "GetDataByCommand", input)
    }
}

/// Firmware feature identifier for `LENOVO_OTHER_METHOD`.
///
/// Reverse engineering (lde_module_fan.dll, 2026-11-01) established the wire
/// layout `(family << 16) | index`: the vendor code builds the identifier as
/// `(member & 0xFFFF) | 0x04030000`, and a read-only rdata table contains
/// `0x80040E50`, `0x80040DF0`, and `0x05040000`. Individual IDs are not yet
/// mapped to named features, so this type only encodes the proven layout.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct FeatureId(u32);

impl FeatureId {
    /// Observed feature families (see type documentation).
    pub const FAMILY_0403: u16 = 0x0403;
    pub const FAMILY_0504: u16 = 0x0504;

    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn from_parts(family: u16, index: u16) -> Self {
        Self(((family as u32) << 16) | (index as u32))
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn family(self) -> u16 {
        (self.0 >> 16) as u16
    }

    #[must_use]
    pub const fn index(self) -> u16 {
        self.0 as u16
    }
}

/// `Is*` probe methods on `LENOVO_GAMEZONE_DATA`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportProbe {
    FanCooling,
    SmartFan,
    CpuOc,
    GpuOc,
    IgpuMode,
    DisableWinKey,
    DisableTouchpad,
    Lighting,
    WaterCooling,
}

impl SupportProbe {
    pub const fn method(self) -> &'static str {
        match self {
            Self::FanCooling => "IsSupportFanCooling",
            Self::SmartFan => "IsSupportSmartFan",
            Self::CpuOc => "IsSupportCpuOC",
            Self::GpuOc => "IsSupportGpuOC",
            Self::IgpuMode => "IsSupportIGPUMode",
            Self::DisableWinKey => "IsSupportDisableWinKey",
            Self::DisableTouchpad => "IsSupportDisableTP",
            Self::Lighting => "IsSupportLightingFeature",
            Self::WaterCooling => "IsSupportWaterCooling",
        }
    }
}

/// Extract the first numeric output property. Prefer the conventional Lenovo
/// payload names, then any unsigned integer; fall back to signed values.
fn first_u32(output: &WmiObject, method: &str) -> Result<u32> {
    for key in ["Data", "Mode", "Status", "Value", "Result", "Output"] {
        if let Some(value) = output.get(key) {
            return numeric(value, method);
        }
    }
    for value in output.values() {
        if let Ok(number) = numeric(value, method) {
            return Ok(number);
        }
    }
    Err(LctrlError::ChannelUnavailable {
        channel: format!("{GAMEZONE_CLASS}.{method} returned no numeric output"),
    })
}

fn numeric(value: &WmiValue, method: &str) -> Result<u32> {
    match value {
        WmiValue::U32(number) => Ok(*number),
        WmiValue::I32(number) => {
            u32::try_from(*number).map_err(|_| LctrlError::ChannelUnavailable {
                channel: format!("{GAMEZONE_CLASS}.{method} returned a negative value"),
            })
        }
        WmiValue::U16(number) => Ok(u32::from(*number)),
        WmiValue::I16(number) => Ok(u32::from(u16::try_from(*number).map_err(|_| {
            LctrlError::ChannelUnavailable {
                channel: format!("{GAMEZONE_CLASS}.{method} returned a negative value"),
            }
        })?)),
        WmiValue::U8(number) => Ok(u32::from(*number)),
        _ => Err(LctrlError::ChannelUnavailable {
            channel: format!("{GAMEZONE_CLASS}.{method} returned a non-numeric value"),
        }),
    }
}

fn require_return_zero(output: &WmiObject, method: &str) -> Result<()> {
    match output.get("ReturnValue") {
        Some(WmiValue::U32(0)) | Some(WmiValue::I32(0)) | None => Ok(()),
        Some(WmiValue::U32(code)) => Err(LctrlError::FirmwareRejected {
            detail: format!("{GAMEZONE_CLASS}.{method} rejected the write: ReturnValue={code}"),
        }),
        Some(WmiValue::I32(code)) => Err(LctrlError::FirmwareRejected {
            detail: format!("{GAMEZONE_CLASS}.{method} rejected the write: ReturnValue={code}"),
        }),
        Some(_) => Err(LctrlError::ChannelUnavailable {
            channel: format!("{GAMEZONE_CLASS}.{method} returned an unusable status"),
        }),
    }
}

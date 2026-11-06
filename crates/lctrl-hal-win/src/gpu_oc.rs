//! GPU overclock status on the Lenovo firmware OC plane.
//!
//! Recovered contract (`docs/evidence/re-gpu-oc-20261105.md`): the support
//! probe `LENOVO_GAMEZONE_DATA.IsSupportGpuOC` is the only GPU-OC verb that
//! answers on 21VG (`ReturnValue=True, Data=0`); neither
//! `LENOVO_GAMEZONE_GPU_OC_DATA` nor `LENOVO_GPU_OVERCLOCKING_DATA`
//! instantiates (`WBEM_E_NOT_SUPPORTED`), and `LENOVO_GPU_METHOD` declares no
//! methods. There is therefore no readable clock/offset/TGP value and no
//! Lenovo WMI setter to wrap: this module exposes the read only and never
//! invents an OC parameter that the firmware did not publish.

use lctrl_core::{GpuOcParameter, GpuOcStatus, GpuOcSupportProbe, LctrlError, Result};
use lctrl_hal::GpuOcControl;

use crate::{GameZone, SupportProbe, WmiObject, WmiTransport, WmiValue, wmi_contract::ROOT_WMI};

/// The vendor consumer's own query (`SEGPUOCPlugin.dll`, recovered verbatim).
const GPU_OC_QUERY: &str = "SELECT * FROM LENOVO_GAMEZONE_GPU_OC_DATA";
const PROBE_METHOD: &str = "LENOVO_GAMEZONE_DATA.IsSupportGpuOC";

/// Read-only GPU overclock service.
pub struct WindowsGpuOc<W> {
    wmi: W,
}

impl<W> WindowsGpuOc<W> {
    #[must_use]
    pub const fn new(wmi: W) -> Self {
        Self { wmi }
    }

    #[must_use]
    pub const fn wmi(&self) -> &W {
        &self.wmi
    }
}

impl<W> WindowsGpuOc<W>
where
    W: WmiTransport,
{
    /// Read the OC parameter block the firmware publishes, if any.
    ///
    /// A missing instance or an explicit `WBEM_E_NOT_SUPPORTED` is returned as
    /// an error; [`GpuOcControl::gpu_oc_status`] keeps that verdict as text
    /// instead of failing the whole status read. More than one row is refused
    /// rather than silently picking one: the recovered contract describes a
    /// single OC parameter block, and which device a second row belongs to is
    /// not established.
    fn parameter(&self) -> Result<GpuOcParameter> {
        let mut rows = self.wmi.query(ROOT_WMI, GPU_OC_QUERY)?;
        if rows.len() > 1 {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "LENOVO_GAMEZONE_GPU_OC_DATA returned {} parameter blocks",
                    rows.len()
                ),
            });
        }
        let row = rows.pop().ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: "LENOVO_GAMEZONE_GPU_OC_DATA returned no instance".into(),
        })?;
        Ok(GpuOcParameter {
            capability: required(&row, "Capability")?,
            clock_id: required(&row, "ClockID")?,
            default_value: required(&row, "defaultvalue")?,
            interval: required(&row, "Interval")?,
            no_oc_order_id: required(&row, "NOCOrderid")?,
            oc_max_offset: required(&row, "OCMaxOffset")?,
            oc_min_offset: required(&row, "OCMinOffset")?,
            oc_offset_freq: required(&row, "OCOffsetFreq")?,
            oc_offset_scale: required(&row, "OCOffsetScale")?,
            oc_order_id: required(&row, "OCOrderid")?,
            pstate_id: required(&row, "PStateID")?,
        })
    }
}

impl<W> GpuOcControl for WindowsGpuOc<W>
where
    W: WmiTransport,
{
    fn gpu_oc_status(&self) -> Result<GpuOcStatus> {
        let zone = GameZone::new(&self.wmi);
        let data = zone.support_data(SupportProbe::GpuOc)?;
        let (parameter, parameter_error) = match self.parameter() {
            Ok(parameter) => (Some(parameter), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Ok(GpuOcStatus {
            supported: data == 1,
            probe: GpuOcSupportProbe {
                method: PROBE_METHOD.into(),
                // `support_data` already rejected a `ReturnValue=False` answer:
                // reaching this point means the provider executed the method.
                return_value: true,
                data,
            },
            parameter,
            parameter_error,
        })
    }
}

fn required(row: &WmiObject, name: &str) -> Result<u32> {
    row.get(name)
        .ok_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("LENOVO_GAMEZONE_GPU_OC_DATA omitted {name}"),
        })
        .and_then(|value| numeric_u32(value, name))
}

fn numeric_u32(value: &WmiValue, name: &str) -> Result<u32> {
    let raw = match value {
        WmiValue::U8(number) => i64::from(*number),
        WmiValue::U16(number) => i64::from(*number),
        WmiValue::U32(number) => i64::from(*number),
        WmiValue::I8(number) => i64::from(*number),
        WmiValue::I16(number) => i64::from(*number),
        WmiValue::I32(number) => i64::from(*number),
        WmiValue::U64(number) => i64::try_from(*number).map_err(|_| invalid(name, value))?,
        WmiValue::I64(number) => *number,
        other => return Err(invalid(name, other)),
    };
    u32::try_from(raw).map_err(|_| invalid(name, value))
}

fn invalid(name: &str, value: &WmiValue) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("LENOVO_GAMEZONE_GPU_OC_DATA {name} has an invalid value: {value:?}"),
    }
}

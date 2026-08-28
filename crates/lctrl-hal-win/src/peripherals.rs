use std::{collections::BTreeMap, time::Duration};

use lctrl_core::{
    ApplyMode, BacklightState, ChangeReport, LctrlError, LightingEffect, PanelRefreshCapability,
    RefreshMode, Result,
};
use lctrl_hal::{KeyboardControl, PanelControl, poll_readback};

use crate::wmi_contract::ROOT_WMI;
use crate::{WmiMethodResult, WmiObject, WmiTransport, WmiValue, active_instance};

const LIGHTING_DATA: &str = "LENOVO_LIGHTING_DATA";
const LIGHTING_METHOD: &str = "LENOVO_LIGHTING_METHOD";
const PANEL_REFRESH_DATA: &str = "LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA";
const KEYBOARD_LIGHTING_ID: u8 = 0;

#[derive(Debug)]
pub struct WindowsPeripheralController<W> {
    transport: W,
}

impl<W> WindowsPeripheralController<W> {
    #[must_use]
    pub const fn new(transport: W) -> Self {
        Self { transport }
    }

    #[must_use]
    pub const fn transport(&self) -> &W {
        &self.transport
    }
}

impl<W> WindowsPeripheralController<W>
where
    W: WmiTransport,
{
    fn backlight_metadata(&self) -> Result<(u8, u8)> {
        let query = format!(
            "SELECT Lighting_Type, State_Type_Num FROM {LIGHTING_DATA} WHERE Lighting_Id = 0"
        );
        let object = self
            .transport
            .query(ROOT_WMI, &query)?
            .into_iter()
            .next()
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "kbd.backlight".into(),
            })?;
        let lighting_type = required_u8(&object, "Lighting_Type")?;
        let count = required_u8(&object, "State_Type_Num")?;
        let max_level = count
            .checked_sub(1)
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: "keyboard backlight reports zero brightness states".into(),
            })?;
        Ok((lighting_type, max_level))
    }

    fn read_backlight(&self) -> Result<BacklightState> {
        let (lighting_type, max_level) = self.backlight_metadata()?;
        let instance = active_instance(&self.transport, LIGHTING_METHOD)?;
        let output = self.transport.invoke_instance(
            ROOT_WMI,
            LIGHTING_METHOD,
            instance.path(),
            "Get_Lighting_Current_Status",
            &BTreeMap::from([("Lighting_ID".into(), WmiValue::U8(KEYBOARD_LIGHTING_ID))]),
        )?;
        let result = WmiMethodResult::parse(output)?;
        result.require_accepted("kbd.backlight.read")?;
        let level = required_u8(result.output(), "Current_Brightness_Level")?;
        let effect = LightingEffect::from_raw(required_u8(result.output(), "Current_State_Type")?);
        if lighting_type <= 1 && !matches!(effect, LightingEffect::Static) {
            return Err(LctrlError::ChannelUnavailable {
                channel: "single-color keyboard reported an unsupported lighting effect".into(),
            });
        }
        BacklightState::new(level, max_level, effect)
    }

    fn write_backlight(&self, state: &BacklightState) -> Result<()> {
        let instance = active_instance(&self.transport, LIGHTING_METHOD)?;
        let output = self.transport.invoke_instance(
            ROOT_WMI,
            LIGHTING_METHOD,
            instance.path(),
            "Set_Lighting_Current_Status",
            &BTreeMap::from([
                ("Current_Brightness_Level".into(), WmiValue::U8(state.level)),
                (
                    "Current_State_Type".into(),
                    WmiValue::U8(state.effect.raw()),
                ),
                ("Lighting_ID".into(), WmiValue::U8(KEYBOARD_LIGHTING_ID)),
            ]),
        )?;
        WmiMethodResult::parse(output)?.require_accepted("kbd.backlight.write")?;
        Ok(())
    }
}

impl<W> KeyboardControl for WindowsPeripheralController<W>
where
    W: WmiTransport,
{
    fn backlight_state(&self) -> Result<BacklightState> {
        self.read_backlight()
    }

    fn set_backlight(
        &self,
        level: u8,
        effect: LightingEffect,
        apply: ApplyMode,
    ) -> Result<ChangeReport<BacklightState>> {
        let previous = self.read_backlight()?;
        let (lighting_type, max_level) = self.backlight_metadata()?;
        if lighting_type <= 1 && !matches!(effect, LightingEffect::Static) {
            return Err(LctrlError::Unsupported {
                feature: "kbd.backlight.effect".into(),
            });
        }
        let requested = BacklightState::new(level, max_level, effect)?;
        if apply == ApplyMode::DryRun {
            return Ok(ChangeReport::dry_run(previous, requested));
        }
        let result = self.write_backlight(&requested).and_then(|()| {
            poll_readback(&requested, 10, Duration::from_millis(50), || {
                self.read_backlight()
            })
        });
        match result {
            Ok(actual) => Ok(ChangeReport::committed(previous, requested, actual)),
            Err(error) => {
                let rollback = self.write_backlight(&previous).and_then(|()| {
                    poll_readback(&previous, 10, Duration::from_millis(50), || {
                        self.read_backlight()
                    })
                    .map(|_| ())
                });
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(LctrlError::FirmwareRejected {
                        detail: format!(
                            "keyboard backlight write failed ({error}); rollback also failed ({rollback})"
                        ),
                    }),
                }
            }
        }
    }
}

impl<W> PanelControl for WindowsPeripheralController<W>
where
    W: WmiTransport,
{
    fn refresh_capability(&self) -> Result<PanelRefreshCapability> {
        let object = self.panel_refresh_object()?;
        Ok(PanelRefreshCapability::new(
            required_u16(&object, "MinimumRefreshRate")?,
            required_u16(&object, "MaximumRefreshRate")?,
            required_u16(&object, "DefaultRefreshRate")?,
        ))
    }

    fn refresh_mode(&self) -> Result<RefreshMode> {
        let object = self.panel_refresh_object()?;
        Ok(RefreshMode::from_raw(required_u16(&object, "Mode")?))
    }
}

impl<W> WindowsPeripheralController<W>
where
    W: WmiTransport,
{
    fn panel_refresh_object(&self) -> Result<WmiObject> {
        let query = format!(
            "SELECT MinimumRefreshRate, MaximumRefreshRate, DefaultRefreshRate, Mode FROM {PANEL_REFRESH_DATA} WHERE Active = TRUE"
        );
        self.transport
            .query(ROOT_WMI, &query)?
            .into_iter()
            .next()
            .ok_or_else(|| LctrlError::Unsupported {
                feature: "panel.refresh".into(),
            })
    }
}

fn required_u8(object: &WmiObject, name: &str) -> Result<u8> {
    match object.get(name) {
        Some(WmiValue::U8(value)) => Ok(*value),
        Some(other) => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI {name} is not u8: {other:?}"),
        }),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI omitted {name}"),
        }),
    }
}

fn required_u16(object: &WmiObject, name: &str) -> Result<u16> {
    match object.get(name) {
        Some(WmiValue::U16(value)) => Ok(*value),
        Some(other) => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI {name} is not u16: {other:?}"),
        }),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("WMI omitted {name}"),
        }),
    }
}

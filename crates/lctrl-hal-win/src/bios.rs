use std::collections::BTreeMap;

use lctrl_core::{
    BiosChange, BiosItem, BiosPasswordStatus, LctrlError, Result, is_success,
    parse_current_setting, parse_selections, save_parameter,
};
use lctrl_hal::BiosControl;

use crate::wmi_contract::ROOT_WMI;
use crate::{WmiMethodResult, WmiObject, WmiTransport, WmiValue};

const BIOS_SETTING_CLASS: &str = "Lenovo_BiosSetting";
const BIOS_SET_CLASS: &str = "Lenovo_SetBiosSetting";
const BIOS_SAVE_CLASS: &str = "Lenovo_SaveBiosSettings";
const BIOS_SELECTIONS_CLASS: &str = "Lenovo_GetBiosSelections";
const BIOS_PASSWORD_CLASS: &str = "Lenovo_BiosPasswordSettings";

#[derive(Debug)]
pub struct WindowsBiosController<W> {
    transport: W,
}

impl<W> WindowsBiosController<W> {
    #[must_use]
    pub const fn new(transport: W) -> Self {
        Self { transport }
    }

    #[must_use]
    pub const fn transport(&self) -> &W {
        &self.transport
    }
}

impl<W> BiosControl for WindowsBiosController<W>
where
    W: WmiTransport,
{
    fn list(&self) -> Result<Vec<BiosItem>> {
        let query = format!("SELECT CurrentSetting FROM {BIOS_SETTING_CLASS}");
        let objects = self.transport.query(ROOT_WMI, &query)?;
        Ok(objects
            .iter()
            .filter_map(|object| match object.get("CurrentSetting") {
                Some(WmiValue::String(setting)) => parse_current_setting(setting),
                _ => None,
            })
            .collect())
    }

    fn get(&self, name: &str) -> Result<Option<BiosItem>> {
        Ok(self
            .list()?
            .into_iter()
            .find(|item| item.name.eq_ignore_ascii_case(name)))
    }

    fn selections(&self, name: &str) -> Result<Vec<String>> {
        let output = self.invoke(
            BIOS_SELECTIONS_CLASS,
            "GetBiosSelections",
            BTreeMap::from([("Item".into(), WmiValue::String(name.into()))]),
        )?;
        let result = WmiMethodResult::parse(output)?;
        result.require_accepted("bios.selections")?;
        match result.output().get("Selections") {
            Some(WmiValue::String(selections)) => Ok(parse_selections(selections)),
            Some(other) => Err(LctrlError::ChannelUnavailable {
                channel: format!("BIOS selections returned non-string value: {other:?}"),
            }),
            None => Err(LctrlError::ChannelUnavailable {
                channel: "BIOS selections omitted Selections".into(),
            }),
        }
    }

    fn stage(&self, change: BiosChange) -> Result<()> {
        let output = self.invoke(
            BIOS_SET_CLASS,
            "SetBiosSetting",
            BTreeMap::from([("parameter".into(), WmiValue::String(change.serialized()))]),
        )?;
        require_business_success(output, "bios.stage")
    }

    fn save(&self) -> Result<()> {
        let output = self.invoke(
            BIOS_SAVE_CLASS,
            "SaveBiosSettings",
            BTreeMap::from([(
                "parameter".into(),
                WmiValue::String(save_parameter().into()),
            )]),
        )?;
        require_business_success(output, "bios.save")
    }

    fn password_status(&self) -> Result<BiosPasswordStatus> {
        let query =
            format!("SELECT MinLength, MaxLength, PasswordState FROM {BIOS_PASSWORD_CLASS}");
        let objects = self.transport.query(ROOT_WMI, &query)?;
        let object = objects.first().ok_or_else(|| LctrlError::Unsupported {
            feature: "bios.password.status".into(),
        })?;
        Ok(BiosPasswordStatus::from_raw(
            required_u32(object, "MinLength")?,
            required_u32(object, "MaxLength")?,
            required_u32(object, "PasswordState")?,
        ))
    }
}

impl<W> WindowsBiosController<W>
where
    W: WmiTransport,
{
    fn invoke(&self, class: &str, method: &str, input: WmiObject) -> Result<WmiObject> {
        let path = first_method_path(&self.transport, class)?;
        self.transport
            .invoke_instance(ROOT_WMI, class, &path, method, &input)
    }
}

fn first_method_path(transport: &dyn WmiTransport, class: &str) -> Result<String> {
    let objects = transport.query(ROOT_WMI, &format!("SELECT __Path FROM {class}"))?;
    objects
        .iter()
        .find_map(|object| match object.get("__Path") {
            Some(WmiValue::String(path)) if !path.is_empty() => Some(path.clone()),
            _ => None,
        })
        .ok_or_else(|| LctrlError::Unsupported {
            feature: format!("wmi.class.{class}"),
        })
}

fn require_business_success(output: WmiObject, feature: &str) -> Result<()> {
    let result = WmiMethodResult::parse(output)?;
    result.require_accepted(feature)?;
    let returned = result
        .output()
        .get("return")
        .or_else(|| result.output().get("Return"));
    match returned {
        Some(WmiValue::String(value)) if is_success(value) => Ok(()),
        Some(WmiValue::String(value)) => Err(LctrlError::FirmwareRejected {
            detail: format!("{feature} rejected: {value}"),
        }),
        Some(other) => Err(LctrlError::ChannelUnavailable {
            channel: format!("{feature} returned non-string business result: {other:?}"),
        }),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("{feature} omitted business return value"),
        }),
    }
}

fn required_u32(object: &WmiObject, name: &str) -> Result<u32> {
    match object.get(name) {
        Some(WmiValue::U32(value)) => Ok(*value),
        Some(other) => Err(LctrlError::ChannelUnavailable {
            channel: format!("BIOS {name} is not u32: {other:?}"),
        }),
        None => Err(LctrlError::ChannelUnavailable {
            channel: format!("BIOS password status omitted {name}"),
        }),
    }
}

use std::collections::BTreeMap;

use lctrl_core::{
    BiosAssistantCapabilities, BiosAssistantValue, BiosChange, BiosItem, BiosPasswordStatus,
    LctrlError, Result, is_success, parse_current_setting, parse_selections, save_parameter,
};
use lctrl_hal::{BiosAssistantControl, BiosControl};

use crate::wmi_contract::ROOT_WMI;
use crate::{WmiMethodResult, WmiObject, WmiTransport, WmiValue};

const BIOS_SETTING_CLASS: &str = "Lenovo_BiosSetting";
const BIOS_SET_CLASS: &str = "Lenovo_SetBiosSetting";
const BIOS_SAVE_CLASS: &str = "Lenovo_SaveBiosSettings";
const BIOS_DISCARD_CLASS: &str = "Lenovo_DiscardBiosSettings";
const BIOS_DEFAULTS_CLASS: &str = "Lenovo_LoadDefaultSettings";
const BIOS_PASSWORD_SET_CLASS: &str = "Lenovo_SetBiosPassword";
const BIOS_SELECTIONS_CLASS: &str = "Lenovo_GetBiosSelections";
const BIOS_PASSWORD_CLASS: &str = "Lenovo_BiosPasswordSettings";
const BIOS_ASSISTANT_CLASS: &str = "LENOVO_BIOS_ASSISTANT";

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
    fn require_staging_contract(&self) -> Result<()> {
        if self.transport.bios_staging_contract_verified() {
            Ok(())
        } else {
            Err(LctrlError::ChannelUnavailable {
                channel: "BIOS staging-buffer ownership contract is unverified".into(),
            })
        }
    }

    fn list(&self) -> Result<Vec<BiosItem>> {
        let mut items = self.settings()?;
        for item in &mut items {
            if let Ok(selections) = self.selections(&item.name) {
                item.selections = selections;
            }
        }
        Ok(items)
    }

    fn get(&self, name: &str) -> Result<Option<BiosItem>> {
        Ok(self
            .settings()?
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
        self.require_staging_contract()?;
        let output = self.invoke(
            BIOS_SET_CLASS,
            "SetBiosSetting",
            BTreeMap::from([("parameter".into(), WmiValue::String(change.serialized()))]),
        )?;
        require_business_success(output, "bios.stage")
    }

    fn save(&self) -> Result<()> {
        self.require_staging_contract()?;
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

    fn discard(&self) -> Result<()> {
        self.require_staging_contract()?;
        let output = self.invoke(
            BIOS_DISCARD_CLASS,
            "DiscardBiosSettings",
            BTreeMap::from([(
                "parameter".into(),
                WmiValue::String(save_parameter().into()),
            )]),
        )?;
        require_business_success(output, "bios.discard")
    }

    fn password_status(&self) -> Result<BiosPasswordStatus> {
        let query = format!(
            "SELECT MinLength, MaxLength, PasswordMode, PasswordState, SupportedEncodings, SupportedKeyboard FROM {BIOS_PASSWORD_CLASS}"
        );
        let objects = self.transport.query(ROOT_WMI, &query)?;
        let object = objects.first().ok_or_else(|| LctrlError::Unsupported {
            feature: "bios.password.status".into(),
        })?;
        Ok(BiosPasswordStatus::from_raw_metadata(
            required_u32(object, "MinLength")?,
            required_u32(object, "MaxLength")?,
            required_u32(object, "PasswordMode")?,
            required_u32(object, "PasswordState")?,
            required_u32(object, "SupportedEncodings")?,
            required_u32(object, "SupportedKeyboard")?,
        ))
    }

    fn load_defaults(&self, password: Option<&str>) -> Result<()> {
        // Wire format per the Lenovo WMI deployment guide:
        // "Password,Encoding,KbdLang;" — the password field is empty while no
        // supervisor password is set.
        let parameter = format!("{},ascii,us;", password.unwrap_or_default());
        let output = self.invoke(
            BIOS_DEFAULTS_CLASS,
            "LoadDefaultSettings",
            BTreeMap::from([("parameter".into(), WmiValue::String(parameter))]),
        )?;
        require_business_success(output, "bios.defaults")
    }

    fn verify_password(&self, password: &str) -> Result<bool> {
        // Guard: with no supervisor password set, a same-value set would
        // CREATE one rather than verify it. Refuse before touching the
        // write method so verification stays read-equivalent.
        if !self.password_status()?.supervisor_set {
            return Err(LctrlError::InvalidArgument {
                detail: "no supervisor password is set; verification is undefined".into(),
            });
        }
        let output = self.invoke(
            BIOS_PASSWORD_SET_CLASS,
            "SetBiosPassword",
            BTreeMap::from([(
                "Parameter".into(),
                WmiValue::String(set_password_parameter(password, password)),
            )]),
        )?;
        let result = WmiMethodResult::parse(output)?;
        result.require_accepted("bios.password.verify")?;
        match result
            .output()
            .get("return")
            .or_else(|| result.output().get("Return"))
        {
            Some(WmiValue::String(value)) if is_success(value) => Ok(true),
            // "Failed" is the firmware's wrong-credential verdict; every
            // other string is a contract failure, not a negative answer.
            Some(WmiValue::String(value)) if value == "Failed" => Ok(false),
            Some(WmiValue::String(value)) => Err(LctrlError::FirmwareRejected {
                detail: format!("bios.password.verify rejected: {value}"),
            }),
            Some(other) => Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "bios.password.verify returned non-string business result: {other:?}"
                ),
            }),
            None => Err(LctrlError::ChannelUnavailable {
                channel: "bios.password.verify omitted business return value".into(),
            }),
        }
    }
}

impl<W> BiosAssistantControl for WindowsBiosController<W>
where
    W: WmiTransport,
{
    fn assistant_capabilities(&self) -> Result<BiosAssistantCapabilities> {
        let raw = self.invoke_assistant(
            "GetCapabilityValue",
            WmiObject::new(),
            "bios.assistant.capabilities",
        )?;
        Ok(BiosAssistantCapabilities::from_raw(raw))
    }

    fn assistant_value(&self, index_data: u32) -> Result<BiosAssistantValue> {
        let raw = self.invoke_assistant(
            "GetValue",
            BTreeMap::from([("IndexData".into(), WmiValue::U32(index_data))]),
            "bios.assistant.value",
        )?;
        Ok(BiosAssistantValue::from_raw(index_data, raw))
    }
}

impl<W> WindowsBiosController<W>
where
    W: WmiTransport,
{
    fn settings(&self) -> Result<Vec<BiosItem>> {
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

    fn invoke(&self, class: &str, method: &str, input: WmiObject) -> Result<WmiObject> {
        let path = first_method_path(&self.transport, class)?;
        self.transport
            .invoke_instance(ROOT_WMI, class, &path, method, &input)
    }

    fn invoke_assistant(&self, method: &str, input: WmiObject, feature: &str) -> Result<u32> {
        let output = self.invoke(BIOS_ASSISTANT_CLASS, method, input)?;
        let result = WmiMethodResult::parse(output)?;
        result.require_accepted(feature)?;
        let raw = result.data_u32()?;
        if raw & 0x8000_0000 == 0 {
            return Err(LctrlError::FirmwareRejected {
                detail: format!("{feature} command status was not accepted: Data={raw:#010x}"),
            });
        }
        Ok(raw)
    }
}

fn first_method_path(transport: &dyn WmiTransport, class: &str) -> Result<String> {
    Ok(crate::active_instance(transport, class)?.path().to_owned())
}

/// Serialise a `Lenovo_SetBiosPassword.SetBiosPassword` parameter. The
/// Lenovo WMI deployment guide defines the grammar as
/// "PasswordType,CurrentPassword,NewPassword,Encoding,KbdLang;" where the
/// only password type exposed by this contract is the supervisor password
/// (`pap`, matching `PasswordMode` 1 on consumer firmware).
fn set_password_parameter(current: &str, new: &str) -> String {
    format!("pap,{current},{new},ascii,us;")
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

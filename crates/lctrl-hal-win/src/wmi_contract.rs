use std::collections::BTreeMap;

use lctrl_core::{LctrlError, Result};

pub const ROOT_WMI: &str = "ROOT\\WMI";

#[derive(Clone, Debug, PartialEq)]
pub enum WmiValue {
    Empty,
    Null,
    String(String),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Array(Vec<Self>),
}

pub type WmiObject = BTreeMap<String, WmiValue>;

pub trait WmiTransport: Send + Sync {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>>;

    fn invoke_instance(
        &self,
        namespace: &str,
        class: &str,
        object_path: &str,
        method: &str,
        input: &WmiObject,
    ) -> Result<WmiObject>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmiInstance {
    class: String,
    path: String,
}

impl WmiInstance {
    #[must_use]
    pub fn class(&self) -> &str {
        &self.class
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

pub fn active_instance(transport: &dyn WmiTransport, class: &str) -> Result<WmiInstance> {
    validate_class_identifier(class)?;
    let query = format!("SELECT __Path, Active, InstanceName FROM {class} WHERE Active = TRUE");
    let objects = transport.query(ROOT_WMI, &query)?;
    let object = objects
        .into_iter()
        .next()
        .ok_or_else(|| LctrlError::Unsupported {
            feature: format!("wmi.class.{class}"),
        })?;
    if let Some(WmiValue::Bool(false)) = object.get("Active") {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("{ROOT_WMI} class {class} returned an inactive instance"),
        });
    }
    let path = match object.get("__Path") {
        Some(WmiValue::String(path)) if !path.is_empty() => path.clone(),
        _ => {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{ROOT_WMI} class {class} omitted string __Path"),
            });
        }
    };

    Ok(WmiInstance {
        class: class.into(),
        path,
    })
}

fn validate_class_identifier(class: &str) -> Result<()> {
    let valid = !class.is_empty()
        && class
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    if valid {
        return Ok(());
    }
    Err(LctrlError::InvalidArgument {
        detail: format!("invalid WMI class identifier: {class:?}"),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct WmiMethodResult {
    accepted: bool,
    output: WmiObject,
}

impl WmiMethodResult {
    pub fn parse(mut output: WmiObject) -> Result<Self> {
        let accepted = match output.remove("ReturnValue") {
            Some(WmiValue::Bool(value)) => value,
            Some(other) => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("WMI method returned non-Boolean ReturnValue: {other:?}"),
                });
            }
            None => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: "WMI method omitted ReturnValue".into(),
                });
            }
        };
        Ok(Self { accepted, output })
    }

    pub fn require_accepted(&self, feature: &str) -> Result<()> {
        if self.accepted {
            return Ok(());
        }
        Err(LctrlError::FirmwareRejected {
            detail: format!("WMI firmware rejected {feature}"),
        })
    }

    pub fn require_zero_status(&self, feature: &str) -> Result<u32> {
        self.require_accepted(feature)?;
        let status = self.data_u32()?;
        if status == 0 {
            return Ok(status);
        }
        Err(LctrlError::FirmwareRejected {
            detail: format!("WMI {feature} returned status {status}"),
        })
    }

    pub fn data_u32(&self) -> Result<u32> {
        match self.output.get("Data") {
            Some(WmiValue::U32(value)) => Ok(*value),
            Some(other) => Err(LctrlError::ChannelUnavailable {
                channel: format!("WMI method returned non-u32 Data: {other:?}"),
            }),
            None => Err(LctrlError::ChannelUnavailable {
                channel: "WMI method omitted Data".into(),
            }),
        }
    }

    #[must_use]
    pub fn output(&self) -> &WmiObject {
        &self.output
    }
}

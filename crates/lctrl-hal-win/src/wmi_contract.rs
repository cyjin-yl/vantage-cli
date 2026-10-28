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
    /// True only with an independently verified target-wide staging isolation
    /// contract, not merely because WMI is reachable.
    fn bios_staging_isolation_verified(&self) -> bool {
        false
    }

    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>>;

    /// An empty output means the transport completed without returned properties
    /// (possibly without an output object); it does not prove a device effect.
    /// Callers requiring return data must
    /// reject its absence, and outputless mutations require actual readback.
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
    if objects.is_empty() {
        return Err(LctrlError::Unsupported {
            feature: format!("wmi.class.{class}"),
        });
    }
    // Firmware providers can answer a filtered SELECT with extra rows that carry
    // no instance key, and those rows cannot be invoked. Continue to the first
    // row that has a usable key instead of failing the whole channel.
    let mut inactive = false;
    let mut missing_key = None;
    for object in objects {
        if let Some(WmiValue::Bool(false)) = object.get("Active") {
            inactive = true;
            continue;
        }
        match instance_path(class, &object) {
            Ok(path) => {
                return Ok(WmiInstance {
                    class: class.into(),
                    path,
                });
            }
            Err(error) => {
                missing_key.get_or_insert(error);
            }
        }
    }
    if inactive && missing_key.is_none() {
        return Err(LctrlError::ChannelUnavailable {
            channel: format!("{ROOT_WMI} class {class} returned an inactive instance"),
        });
    }
    Err(
        missing_key.unwrap_or_else(|| LctrlError::ChannelUnavailable {
            channel: format!("{ROOT_WMI} class {class} returned an inactive instance"),
        }),
    )
}

fn instance_path(class: &str, object: &WmiObject) -> Result<String> {
    let path = match object.get("__Path") {
        Some(WmiValue::String(path)) if !path.is_empty() => path.clone(),
        _ => match object.get("InstanceName") {
            Some(WmiValue::String(instance))
                if !instance.is_empty() && !instance.contains('\0') =>
            {
                // These firmware-backed classes do not expose a usable __Path
                // system property. Their InstanceName is the instance key used
                // by the official ManagementObject invocation form.
                format!("{class}.InstanceName={}", quote_wql(instance))
            }
            _ => {
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!(
                        "{ROOT_WMI} class {class} omitted both __Path and InstanceName"
                    ),
                });
            }
        },
    };
    Ok(path)
}

fn quote_wql(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
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

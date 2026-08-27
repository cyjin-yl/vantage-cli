use std::{collections::HashMap, io};

use lctrl_core::{LctrlError, Result};
use wmi::{IWbemClassWrapper, Variant, WMIConnection, WMIError};

use crate::{WmiObject, WmiTransport, WmiValue, map_wmi_hresult};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWmi;

impl WmiTransport for NativeWmi {
    fn query(&self, namespace: &str, wql: &str) -> Result<Vec<WmiObject>> {
        let connection = connect(namespace)?;
        let objects: Vec<HashMap<String, Variant>> = connection
            .raw_query(wql)
            .map_err(|error| map_error(error, &format!("{namespace} query")))?;
        objects.into_iter().map(convert_object).collect()
    }

    fn invoke_instance(
        &self,
        namespace: &str,
        class: &str,
        object_path: &str,
        method: &str,
        input: &WmiObject,
    ) -> Result<WmiObject> {
        let connection = connect(namespace)?;
        let input_instance = if input.is_empty() {
            None
        } else {
            let class_object = connection
                .get_object(class)
                .map_err(|error| map_error(error, &format!("{namespace} class {class}")))?;
            let signature = class_object
                .get_method(method)
                .map_err(|error| map_error(error, &format!("{class}.{method} signature")))?
                .ok_or_else(|| LctrlError::Unsupported {
                    feature: format!("wmi.method.{class}.{method}"),
                })?;
            let instance = signature
                .spawn_instance()
                .map_err(|error| map_error(error, &format!("{class}.{method} input")))?;
            for (name, value) in input {
                instance
                    .put_property(name, to_variant(value))
                    .map_err(|error| map_error(error, &format!("{class}.{method}.{name}")))?;
            }
            Some(instance)
        };

        let output = connection
            .exec_method(object_path, method, input_instance.as_ref())
            .map_err(|error| map_error(error, &format!("{class}.{method}")))?
            .ok_or_else(|| LctrlError::ChannelUnavailable {
                channel: format!("WMI {class}.{method} returned no output object"),
            })?;
        wrapper_to_object(&output, class, method)
    }
}

fn connect(namespace: &str) -> Result<WMIConnection> {
    WMIConnection::with_namespace_path(namespace)
        .map_err(|error| map_error(error, &format!("WMI namespace {namespace}")))
}

fn wrapper_to_object(wrapper: &IWbemClassWrapper, class: &str, method: &str) -> Result<WmiObject> {
    let mut output = WmiObject::new();
    let properties = wrapper
        .list_properties()
        .map_err(|error| map_error(error, &format!("{class}.{method} output properties")))?;
    for name in properties {
        let value = wrapper
            .get_property(&name)
            .map_err(|error| map_error(error, &format!("{class}.{method}.{name}")))?;
        output.insert(name, from_variant(value)?);
    }
    Ok(output)
}

fn convert_object(object: HashMap<String, Variant>) -> Result<WmiObject> {
    object
        .into_iter()
        .map(|(name, value)| Ok((name, from_variant(value)?)))
        .collect()
}

fn from_variant(value: Variant) -> Result<WmiValue> {
    Ok(match value {
        Variant::Empty => WmiValue::Empty,
        Variant::Null => WmiValue::Null,
        Variant::String(value) => WmiValue::String(value),
        Variant::I1(value) => WmiValue::I8(value),
        Variant::I2(value) => WmiValue::I16(value),
        Variant::I4(value) => WmiValue::I32(value),
        Variant::I8(value) => WmiValue::I64(value),
        Variant::R4(value) => WmiValue::F32(value),
        Variant::R8(value) => WmiValue::F64(value),
        Variant::Bool(value) => WmiValue::Bool(value),
        Variant::UI1(value) => WmiValue::U8(value),
        Variant::UI2(value) => WmiValue::U16(value),
        Variant::UI4(value) => WmiValue::U32(value),
        Variant::UI8(value) => WmiValue::U64(value),
        Variant::Array(values) => WmiValue::Array(
            values
                .into_iter()
                .map(from_variant)
                .collect::<Result<Vec<_>>>()?,
        ),
        Variant::Unknown(_) | Variant::Object(_) => {
            return Err(LctrlError::ChannelUnavailable {
                channel: "WMI returned an unsupported object/unknown VARIANT".into(),
            });
        }
    })
}

fn to_variant(value: &WmiValue) -> Variant {
    match value {
        WmiValue::Empty => Variant::Empty,
        WmiValue::Null => Variant::Null,
        WmiValue::String(value) => Variant::String(value.clone()),
        WmiValue::I8(value) => Variant::I1(*value),
        WmiValue::I16(value) => Variant::I2(*value),
        WmiValue::I32(value) => Variant::I4(*value),
        WmiValue::I64(value) => Variant::I8(*value),
        WmiValue::U8(value) => Variant::UI1(*value),
        WmiValue::U16(value) => Variant::UI2(*value),
        WmiValue::U32(value) => Variant::UI4(*value),
        WmiValue::U64(value) => Variant::UI8(*value),
        WmiValue::F32(value) => Variant::R4(*value),
        WmiValue::F64(value) => Variant::R8(*value),
        WmiValue::Bool(value) => Variant::Bool(*value),
        WmiValue::Array(values) => Variant::Array(values.iter().map(to_variant).collect()),
    }
}

fn map_error(error: WMIError, context: &str) -> LctrlError {
    match error {
        WMIError::HResultError { hres } => map_wmi_hresult(hres, context),
        other => LctrlError::Io(io::Error::other(format!("{context}: {other}"))),
    }
}

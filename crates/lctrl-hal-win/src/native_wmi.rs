use std::{collections::HashMap, io};

use lctrl_core::{LctrlError, Result};
use wmi::{IWbemClassWrapper, Variant, WMIConnection, WMIError};

use crate::{WmiObject, WmiTransport, WmiValue, map_wmi_hresult};

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeWmi;

impl WmiTransport for NativeWmi {
    /// Established on TB14P-EZRA (2026-11-03): the NVRAM staging buffer is a
    /// single machine-global buffer. A `SetBiosSetting` from one WMI client
    /// is committed by a `SaveBiosSettings` issued from a different client
    /// that never staged anything, and `DiscardBiosSettings` clears pending
    /// changes for every client. Save/Discard are therefore disclosed as
    /// machine-wide operations; they are never denied for lack of isolation.
    fn bios_staging_contract_verified(&self) -> bool {
        true
    }

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
        // `get_method` returns None for methods without an in-parameters
        // signature class (all parameterless getters). That is not "method
        // missing": pass NULL in-params in that case. When a signature class
        // exists, always spawn an instance (empty when no properties are set):
        // several Lenovo firmware providers reject a NULL in-params pointer
        // with WBEM_E_INVALID_METHOD_PARAMETERS while accepting an empty
        // spawned instance.
        let class_object = connection
            .get_object(class)
            .map_err(|error| map_error(error, &format!("{namespace} class {class}")))?;
        let signature = class_object
            .get_method(method)
            .map_err(|error| map_error(error, &format!("{class}.{method} signature")))?;
        let input_instance = match signature {
            Some(signature) => {
                let instance = signature
                    .spawn_instance()
                    .map_err(|error| map_error(error, &format!("{class}.{method} input")))?;
                for (name, value) in input {
                    instance
                        .put_property(name, to_variant(value))
                        .map_err(|error| map_error(error, &format!("{class}.{method}.{name}")))?;
                }
                Some(instance)
            }
            None => None,
        };

        let output = connection
            .exec_method(object_path, method, input_instance.as_ref())
            .map_err(|error| map_error(error, &format!("{class}.{method}")))?;
        match output {
            Some(output) => wrapper_to_object(&output, class, method),
            None => Ok(WmiObject::new()),
        }
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

#[cfg(test)]
mod tests {
    use super::{from_variant, to_variant};
    use crate::WmiValue;

    #[test]
    fn supported_values_round_trip_through_native_variants() {
        let values = [
            WmiValue::Empty,
            WmiValue::Null,
            WmiValue::String("value".into()),
            WmiValue::I8(-1),
            WmiValue::I16(-2),
            WmiValue::I32(-3),
            WmiValue::I64(-4),
            WmiValue::U8(1),
            WmiValue::U16(2),
            WmiValue::U32(3),
            WmiValue::U64(4),
            WmiValue::F32(1.5),
            WmiValue::F64(2.5),
            WmiValue::Bool(true),
            WmiValue::Array(vec![WmiValue::U8(1), WmiValue::U8(2)]),
        ];

        for value in values {
            assert_eq!(from_variant(to_variant(&value)).unwrap(), value);
        }
    }
}

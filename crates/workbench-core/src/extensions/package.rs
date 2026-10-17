use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeSet;
pub(super) const MAX_PACKAGE: usize = 12 * 1024 * 1024;
const MAX_EXECUTABLE: usize = 8 * 1024 * 1024;
const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) struct Package {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub manifest: Value,
}
impl Package {
    pub fn description(&self) -> Value {
        json!({"package_sha256":self.digest,"package_bytes":self.bytes.len(),"manifest":self.manifest,
            "enabled":false,"execution_available":false,"provenance_verified":false})
    }
    pub fn identity(&self) -> (&str, u64) {
        (
            self.manifest["id"].as_str().unwrap(),
            self.manifest["version"].as_u64().unwrap(),
        )
    }
}
fn integer(value: &Value, max: u64) -> bool {
    value.as_u64().is_some_and(|n| n <= max)
}
fn primitive(schema: &Value) -> bool {
    match schema["type"].as_str() {
        Some("string") => {
            keys(schema, &["type", "minLength", "maxLength"], &[])
                && integer(&schema["minLength"], 4096)
                && integer(&schema["maxLength"], 4096)
                && schema["minLength"].as_u64() <= schema["maxLength"].as_u64()
        }
        Some("integer") => {
            keys(schema, &["type", "minimum", "maximum"], &[])
                && schema["minimum"]
                    .as_i64()
                    .is_some_and(|n| (-SAFE_INTEGER..=SAFE_INTEGER).contains(&n))
                && schema["maximum"]
                    .as_i64()
                    .is_some_and(|n| (-SAFE_INTEGER..=SAFE_INTEGER).contains(&n))
                && schema["minimum"].as_i64() <= schema["maximum"].as_i64()
        }
        Some("boolean") => keys(schema, &["type"], &[]),
        _ => false,
    }
}
fn identifier(name: &str) -> bool {
    let mut chars = name.bytes();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}
fn schema(value: &Value) -> bool {
    if !keys(
        value,
        &[
            "$schema",
            "type",
            "additionalProperties",
            "properties",
            "required",
        ],
        &[],
    ) || value["$schema"] != "https://json-schema.org/draft/2020-12/schema"
        || value["type"] != "object"
        || value["additionalProperties"] != false
    {
        return false;
    }
    let Some(properties) = value["properties"].as_object() else {
        return false;
    };
    let Some(required) = value["required"].as_array() else {
        return false;
    };
    let mut unique = BTreeSet::new();
    properties.len() <= 16
        && required.len() == properties.len()
        && properties
            .iter()
            .all(|(name, spec)| identifier(name) && primitive(spec))
        && required.iter().all(|name| {
            name.as_str()
                .is_some_and(|s| properties.contains_key(s) && unique.insert(s))
        })
}
fn elf(bytes: &[u8]) -> bool {
    bytes.len() >= 64
        && &bytes[..4] == b"\x7fELF"
        && bytes[4..7] == [2, 1, 1]
        && matches!(bytes[7], 0 | 3)
        && bytes[8] == 0
        && matches!(u16::from_le_bytes([bytes[16], bytes[17]]), 2 | 3)
        && bytes[18..20] == [62, 0]
        && bytes[20..24] == [1, 0, 0, 0]
        && bytes[52..54] == [64, 0]
}
pub(super) fn decode(
    bytes: Vec<u8>,
    control: &RunControl,
    deadline: Instant,
) -> Result<Package, Fault> {
    active(control, deadline)?;
    let invalid = || Fault::new("invalid_extension_package");
    if bytes.len() > MAX_PACKAGE {
        return Err(Fault::new("extension_package_too_large"));
    }
    let value = crate::request::parse_bounded_json(&bytes, 16).map_err(|_| invalid())?;
    active(control, deadline)?;
    if !keys(
        &value,
        &["package_format", "manifest", "executable_base64"],
        &[],
    ) || value["package_format"] != 1
    {
        return Err(invalid());
    }
    let manifest = &value["manifest"];
    if !keys(
        manifest,
        &[
            "id",
            "version",
            "protocol",
            "compatibility",
            "executable",
            "capabilities",
            "provenance",
        ],
        &[],
    ) || serde_json::to_vec(manifest).map_err(|_| invalid())?.len() > 64 * 1024
        || manifest["id"]
            .as_str()
            .is_none_or(|id| id.len() > 64 || !crate::request::valid_operation(id))
        || manifest["version"]
            .as_u64()
            .is_none_or(|v| !(1..=1_000_000).contains(&v))
        || !keys(&manifest["protocol"], &["name", "version"], &[])
        || manifest["protocol"]["name"] != "save-extension"
        || manifest["protocol"]["version"] != 1
        || !keys(&manifest["compatibility"], &["min_spec", "max_spec"], &[])
        || manifest["compatibility"]["min_spec"] != "0.1"
        || manifest["compatibility"]["max_spec"] != "0.1"
    {
        return Err(invalid());
    }
    let executable = &manifest["executable"];
    if !keys(executable, &["platform", "format", "sha256", "bytes"], &[])
        || executable["platform"] != "linux-x86_64"
        || executable["format"] != "elf"
        || executable["sha256"]
            .as_str()
            .is_none_or(|s| !digest_string(s))
        || executable["bytes"]
            .as_u64()
            .is_none_or(|n| !(1..=MAX_EXECUTABLE as u64).contains(&n))
    {
        return Err(invalid());
    }
    let provenance = &manifest["provenance"];
    if !keys(provenance, &["source", "revision", "license"], &[])
        || !text(&provenance["source"], 1024)
        || !text(&provenance["revision"], 128)
        || !text(&provenance["license"], 128)
    {
        return Err(invalid());
    }
    let capabilities = manifest["capabilities"].as_array().ok_or_else(invalid)?;
    let prefix = format!("ext.{}.", manifest["id"].as_str().unwrap());
    let mut operations = BTreeSet::new();
    if !(1..=8).contains(&capabilities.len())
        || !capabilities.iter().all(|cap| {
            keys(
                cap,
                &[
                    "operation",
                    "version",
                    "effects",
                    "requested_access",
                    "input_schema",
                    "output_schema",
                ],
                &[],
            ) && cap["operation"].as_str().is_some_and(|op| {
                crate::request::valid_operation(op)
                    && op
                        .strip_prefix(&prefix)
                        .is_some_and(crate::request::valid_operation)
                    && operations.insert(op)
            }) && cap["version"] == 1
                && cap["effects"] == "observe"
                && cap["requested_access"] == json!([])
                && schema(&cap["input_schema"])
                && schema(&cap["output_schema"])
        })
    {
        return Err(invalid());
    }
    let encoded = value["executable_base64"].as_str().ok_or_else(invalid)?;
    if encoded.len() > MAX_EXECUTABLE.div_ceil(3) * 4 {
        return Err(invalid());
    }
    let executable_bytes = STANDARD.decode(encoded).map_err(|_| invalid())?;
    active(control, deadline)?;
    if executable_bytes.len() > MAX_EXECUTABLE
        || executable_bytes.len() as u64 != executable["bytes"].as_u64().unwrap()
        || STANDARD.encode(&executable_bytes) != encoded
        || !elf(&executable_bytes)
        || digest(&executable_bytes) != executable["sha256"].as_str().unwrap()
    {
        return Err(invalid());
    }
    let digest = digest(&bytes);
    active(control, deadline)?;
    Ok(Package {
        bytes,
        digest,
        manifest: manifest.clone(),
    })
}

/// Values for the admitted finite schema subset; no coercion, defaults, refs or hooks.
pub(super) fn validate_values(schema: &Value, value: &Value) -> bool {
    let Some(values) = value.as_object() else {
        return false;
    };
    let properties = schema["properties"].as_object().expect("admitted schema");
    values.len() == properties.len()
        && properties.iter().all(|(name, spec)| {
            let Some(value) = values.get(name) else {
                return false;
            };
            match spec["type"].as_str() {
                Some("string") => value.as_str().is_some_and(|s| {
                    let length = s.chars().count() as u64;
                    length >= spec["minLength"].as_u64().unwrap()
                        && length <= spec["maxLength"].as_u64().unwrap()
                }),
                Some("integer") => value.as_i64().is_some_and(|n| {
                    n >= spec["minimum"].as_i64().unwrap() && n <= spec["maximum"].as_i64().unwrap()
                }),
                Some("boolean") => value.is_boolean(),
                _ => false,
            }
        })
}

pub(super) fn executable_bytes(package: &Package) -> Vec<u8> {
    // Decode only the same already admitted, digest-bound package bytes, never a path reread.
    let value: Value = serde_json::from_slice(&package.bytes).expect("admitted package");
    STANDARD
        .decode(value["executable_base64"].as_str().unwrap())
        .expect("admitted executable")
}

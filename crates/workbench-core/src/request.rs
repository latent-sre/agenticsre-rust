use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Read;

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_DEPTH: usize = 16;
pub const MAX_STREAM_BYTES: usize = 1024 * 1024;
pub const MAX_RESULT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub kind: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            kind: "local".into(),
            id: "workstation".into(),
            environment: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,
            max_output_bytes: MAX_STREAM_BYTES,
            max_items: None,
            concurrency: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub spec_version: String,
    pub request_id: String,
    pub operation: String,
    pub operation_version: u64,
    pub target: Target,
    pub inputs: Value,
    pub limits: Limits,
    pub record: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessInput {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub code: &'static str,
    pub message: &'static str,
    pub unsupported: bool,
}

impl Problem {
    pub fn invalid(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            unsupported: false,
        }
    }

    pub fn unsupported(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            unsupported: true,
        }
    }
}

impl Request {
    pub fn new(operation: &str, inputs: Value) -> Self {
        Self {
            spec_version: "0.1".into(),
            request_id: crate::result::new_id("request"),
            operation: operation.into(),
            operation_version: 1,
            target: Target::default(),
            inputs,
            limits: Limits::default(),
            record: "never".into(),
        }
    }

    pub fn validate(&self) -> Result<(), Problem> {
        if !valid_operation(&self.operation)
            || !bounded(&self.request_id, 1, 128)
            || self.operation_version == 0
            || !self.inputs.is_object()
            || !matches!(
                self.target.kind.as_str(),
                "local" | "connection" | "service" | "runner"
            )
            || !bounded(&self.target.id, 1, 128)
            || self
                .target
                .environment
                .as_ref()
                .is_some_and(|v| !bounded(v, 1, 128))
            || (self.target.kind == "service" && self.target.environment.is_none())
            || !(1..=300_000).contains(&self.limits.timeout_ms)
            || !(1024..=MAX_RESULT_BYTES).contains(&self.limits.max_output_bytes)
            || self
                .limits
                .max_items
                .is_some_and(|v| !(1..=1000).contains(&v))
            || self
                .limits
                .concurrency
                .is_some_and(|v| !(1..=16).contains(&v))
            || !matches!(self.record.as_str(), "never" | "auto" | "always")
        {
            return Err(Problem::invalid(
                "invalid_request",
                "Request fields do not satisfy the version 0.1 contract.",
            ));
        }
        if self.spec_version != "0.1" || self.operation_version != 1 {
            return Err(Problem::unsupported(
                "unsupported_version",
                "Only envelope version 0.1 and operation version 1 are supported.",
            ));
        }
        let grafana = matches!(
            self.operation.as_str(),
            "grafana.dashboard.get" | "grafana.query"
        );
        let target_supported = if grafana {
            self.target.kind == "connection" && valid_operation(&self.target.id)
        } else {
            self.target.kind == "local" && self.target.id == "workstation"
        };
        if !target_supported || self.target.environment.is_some() {
            return Err(Problem::unsupported(
                "unsupported_target",
                "Grafana requires a named connection; other operations require the local workstation. Environment selectors are unsupported.",
            ));
        }
        if self.record != "never" {
            return Err(Problem::unsupported(
                "unsupported_record_mode",
                "Persistence is unavailable; explicitly select record=never.",
            ));
        }
        if self.limits.max_items.is_some() || self.limits.concurrency.is_some() {
            return Err(Problem::unsupported(
                "unsupported_limits",
                "These operations do not accept max_items or concurrency controls.",
            ));
        }
        Ok(())
    }
}

pub fn bounded(text: &str, min: usize, max: usize) -> bool {
    let length = text.chars().count();
    (min..=max).contains(&length) && !text.contains('\0')
}

pub fn valid_operation(value: &str) -> bool {
    let mut bytes = value.bytes();
    if value.len() > 128 || !bytes.next().is_some_and(|b| b.is_ascii_lowercase()) {
        return false;
    }
    let mut separator = false;
    for b in bytes {
        if b.is_ascii_lowercase() || b.is_ascii_digit() {
            separator = false;
        } else if matches!(b, b'.' | b'_' | b'-') && !separator {
            separator = true;
        } else {
            return false;
        }
    }
    !separator
}

/// Bound bytes and nesting before deserializing. Duplicate keys are rejected at every depth.
pub fn parse_json(mut reader: impl Read) -> Result<Value, Problem> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Problem::invalid("request_read_failed", "The request could not be read."))?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(Problem::invalid(
            "request_too_large",
            "Request files must be at most 64 KiB.",
        ));
    }
    let value = parse_bounded_json(&bytes, MAX_DEPTH)?;
    for (object, key) in [
        (&value["target"], "environment"),
        (&value["limits"], "max_items"),
        (&value["limits"], "concurrency"),
    ] {
        if object.get(key).is_some_and(Value::is_null) {
            return Err(Problem::invalid(
                "invalid_request",
                "Optional control fields must have a value when present.",
            ));
        }
    }
    Ok(value)
}

/// The caller bounds bytes before this shared strict, finite JSON decoder.
pub fn parse_bounded_json(bytes: &[u8], max_depth: usize) -> Result<Value, Problem> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for &byte in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > max_depth {
                        return Err(Problem::invalid(
                            "request_too_deep",
                            "JSON nesting exceeds the allowed number of containers.",
                        ));
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    // Parse directly into the strict envelope after a duplicate-key walk. Deserializing into
    // Value first would silently normalize duplicate control fields to the last value.
    serde_json::from_slice::<UniqueJson>(bytes).map_err(|_| {
        Problem::invalid(
            "invalid_json",
            "Request JSON is malformed or contains duplicate fields.",
        )
    })?;
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Problem::invalid("invalid_json", "Request JSON is malformed."))?;
    Ok(value)
}

pub fn parse_request(reader: impl Read) -> Result<Request, Problem> {
    let value = parse_json(reader)?;
    if !value.is_object()
        || !value["target"].is_object()
        || !value["limits"].is_object()
        || !value["inputs"].is_object()
    {
        return Err(Problem::invalid(
            "invalid_request",
            "The request, target, limits and inputs must be JSON objects.",
        ));
    }
    serde_json::from_value(value).map_err(|_| {
        Problem::invalid(
            "invalid_request",
            "Request fields are missing, unknown or have invalid types.",
        )
    })
}

struct UniqueJson;

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<UniqueJson, A::Error> {
                while sequence.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<UniqueJson, A::Error> {
                let mut keys = HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate key"));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

//! Exact-plan approval for the single granted local Prometheus rule file consumer.
#[cfg(target_os = "linux")]
mod engine;
#[cfg(target_os = "linux")]
mod fs;
#[cfg(target_os = "linux")]
mod ledger;
#[cfg(target_os = "linux")]
pub(crate) use engine::{Service, valid_config};
pub(crate) mod client;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;

pub(crate) fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
pub(crate) fn pin(v: &str) -> bool {
    v.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn keys(v: &Value, required: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == required.len() && required.iter().all(|k| o.contains_key(*k)))
}
fn identifier(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| crate::request::bounded(s, 1, 128) && !s.chars().any(char::is_control))
}
pub(crate) fn decode(v: &str) -> Option<Vec<u8>> {
    if v.len() > 43692 {
        return None;
    }
    let bytes = STANDARD.decode(v).ok()?;
    (bytes.len() <= 32768 && STANDARD.encode(&bytes) == v).then_some(bytes)
}
pub(crate) fn valid_input(method: &str, v: &Value) -> bool {
    let method = method.strip_prefix("change.").unwrap_or(method);
    match method {
        "plan" => {
            keys(
                v,
                &[
                    "request_id",
                    "target",
                    "proposed_base64",
                    "expires_in_seconds",
                ],
            ) && identifier(&v["request_id"])
                && v["target"]
                    .as_str()
                    .is_some_and(crate::request::valid_operation)
                && v["proposed_base64"].as_str().and_then(decode).is_some()
                && v["expires_in_seconds"]
                    .as_u64()
                    .is_some_and(|s| (1..=900).contains(&s))
        }
        "status" | "reconcile" => {
            keys(v, &["requester", "request_id"])
                && v["requester"]
                    .as_str()
                    .is_some_and(crate::request::valid_operation)
                && identifier(&v["request_id"])
        }
        "approve" => {
            keys(v, &["requester", "request_id", "plan_sha256"])
                && v["requester"]
                    .as_str()
                    .is_some_and(crate::request::valid_operation)
                && identifier(&v["request_id"])
                && v["plan_sha256"].as_str().is_some_and(pin)
        }
        "apply" => {
            keys(v, &["request_id", "plan_sha256", "approval_id"])
                && identifier(&v["request_id"])
                && v["approval_id"].as_str().is_some_and(pin)
                && v["plan_sha256"].as_str().is_some_and(pin)
        }
        _ => false,
    }
}

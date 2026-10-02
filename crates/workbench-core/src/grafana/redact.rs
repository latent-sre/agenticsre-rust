use super::Error;
use crate::request::MAX_RESULT_BYTES;
use base64::{Engine, engine::general_purpose::STANDARD};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, sync::LazyLock};

static USERINFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?://)[^/\s@]+@").expect("fixed URL-userinfo expression")
});

// No Debug: the compiled pattern contains private credential variants.
pub(super) struct Redactor {
    pattern: Regex,
    pub replacements: u64,
    remaining: usize,
}

fn encoded(value: &str, plus: bool) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else if plus && byte == b' ' {
            out.push('+');
        } else {
            use std::fmt::Write;
            write!(&mut out, "%{byte:02X}").expect("write to string");
        }
    }
    out
}

impl Redactor {
    pub fn new(secrets: &[String]) -> Result<Self, Error> {
        let mut variants = BTreeSet::new();
        for secret in secrets {
            variants.extend([
                secret.clone(),
                encoded(secret, false),
                encoded(secret, true),
                STANDARD.encode(secret),
            ]);
        }
        let mut variants: Vec<_> = variants.into_iter().collect();
        variants.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let pattern = Regex::new(
            &variants
                .iter()
                .map(|s| regex::escape(s))
                .collect::<Vec<_>>()
                .join("|"),
        )
        .map_err(|_| Error::admission("invalid_credential"))?;
        Ok(Self {
            pattern,
            replacements: 0,
            remaining: MAX_RESULT_BYTES,
        })
    }

    fn reserve(&mut self, count: usize) -> Result<(), Error> {
        self.remaining = self
            .remaining
            .checked_sub(count)
            .ok_or_else(|| Error::failed("redaction_output_too_large"))?;
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<String, Error> {
        let without_userinfo = USERINFO.replace_all(text, "$1[REDACTED]@");
        let mut out = String::new();
        let mut offset = 0;
        for found in self.pattern.find_iter(&without_userinfo) {
            let needed = found.start() - offset + "[REDACTED]".len();
            if out.len() + needed > MAX_RESULT_BYTES {
                return Err(Error::failed("redaction_output_too_large"));
            }
            out.push_str(&without_userinfo[offset..found.start()]);
            out.push_str("[REDACTED]");
            offset = found.end();
        }
        if out.len() + without_userinfo.len() - offset > MAX_RESULT_BYTES {
            return Err(Error::failed("redaction_output_too_large"));
        }
        out.push_str(&without_userinfo[offset..]);
        if out != text {
            self.replacements += 1;
        }
        self.reserve(out.len())?;
        Ok(out)
    }

    /// Observational metadata is typed T | {redacted:true}; public envelope identity is not passed here.
    pub fn metadata(&mut self, value: Value) -> Value {
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if self.pattern.is_match(&text) || USERINFO.is_match(&text) {
            self.replacements += 1;
            json!({"redacted":true})
        } else {
            value
        }
    }

    pub fn payload(&mut self, value: Value) -> Result<Value, Error> {
        self.reserve(2)?;
        Ok(match value {
            Value::String(text) => Value::String(self.text(&text)?),
            Value::Array(values) => Value::Array(
                values
                    .into_iter()
                    .map(|v| self.payload(v))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Object(values) => {
                let mut output = Map::new();
                for (key, value) in values {
                    let header = matches!(
                        key.to_ascii_lowercase().as_str(),
                        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
                    );
                    let key = self.text(&key)?;
                    if output.contains_key(&key) {
                        return Err(Error::failed("redaction_key_collision"));
                    }
                    let value = if header {
                        self.replacements += 1;
                        json!({"redacted":true})
                    } else {
                        self.payload(value)?
                    };
                    output.insert(key, value);
                }
                Value::Object(output)
            }
            scalar => self.metadata(scalar),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_primitives_encodings_and_key_collisions_are_explicit() {
        let mut redactor =
            Redactor::new(&["741829".into(), "true".into(), "null".into(), "a b".into()]).unwrap();
        let clean = redactor.payload(json!({"number":741829.0,"boolean":true,"null":null,"encoded":"YSBi a%20b a+b", "url":"https://unknown:password@host/path","Authorization":"other-secret"})).unwrap();
        for key in ["number", "boolean", "[REDACTED]", "Authorization"] {
            assert_eq!(clean[key], json!({"redacted":true}));
        }
        assert_eq!(clean["encoded"], "[REDACTED] [REDACTED] [REDACTED]");
        assert_eq!(clean["url"], "https://[REDACTED]@host/path");
        let error = redactor.payload(json!({"741829":1,"true":2})).unwrap_err();
        assert_eq!(error.code, "redaction_key_collision");
    }
}

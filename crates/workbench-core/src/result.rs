use crate::request::{
    Limits, MAX_RESULT_BYTES, MAX_STREAM_BYTES, Problem, Request, Target, bounded, valid_operation,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Succeeded,
    Failed,
    Partial,
    Denied,
    Unsupported,
    Cancelled,
    TimedOut,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct Execution {
    pub status: Status,
    pub child_exit_code: Option<i32>,
    pub signal: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Coverage {
    pub state: String,
    pub scope: String,
    pub limitations: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub encoding: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorRecord {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Serialize)]
pub struct OperationResult {
    pub spec_version: &'static str,
    pub request_id: String,
    pub run_id: String,
    pub operation: String,
    pub operation_version: u64,
    pub target: Target,
    pub resolved_target: Value,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
    pub execution: Execution,
    pub effect_outcome: String,
    pub assessment: &'static str,
    pub coverage: Coverage,
    pub output: Output,
    pub data: Value,
    pub errors: Vec<ErrorRecord>,
    pub sources: Vec<Value>,
    pub artifacts: Vec<Value>,
    pub effective_limits: Limits,
    pub record_mode: &'static str,
}

pub fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub fn new_id(prefix: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{prefix}-{}-{nanos:x}-{:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

impl OperationResult {
    pub fn new(request: &Request) -> Self {
        let now = timestamp();
        let target = if matches!(
            request.target.kind.as_str(),
            "local" | "connection" | "runner" | "service"
        ) && bounded(&request.target.id, 1, 128)
            && request
                .target
                .environment
                .as_ref()
                .is_none_or(|s| bounded(s, 1, 128))
            && (request.target.kind != "service" || request.target.environment.is_some())
        {
            request.target.clone()
        } else {
            Target::default()
        };
        Self {
            spec_version: "0.1",
            request_id: if bounded(&request.request_id, 1, 128) {
                request.request_id.clone()
            } else {
                new_id("request")
            },
            run_id: new_id("run"),
            operation: if valid_operation(&request.operation) {
                request.operation.clone()
            } else {
                "request.invalid".into()
            },
            operation_version: request.operation_version.max(1),
            target,
            resolved_target: json!({}),
            started_at: now.clone(),
            finished_at: now,
            duration_ms: 0,
            execution: Execution {
                status: Status::Succeeded,
                child_exit_code: None,
                signal: None,
            },
            effect_outcome: "not_attempted".into(),
            assessment: "not_assessed",
            coverage: Coverage {
                state: "not_applicable".into(),
                scope: "This local operation only; no service health assessment".into(),
                limitations: vec![],
            },
            output: Output {
                encoding: "utf-8".into(),
                ..Output::default()
            },
            data: json!({}),
            errors: vec![],
            sources: vec![],
            artifacts: vec![],
            effective_limits: Limits {
                timeout_ms: request.limits.timeout_ms.clamp(1, 300_000),
                max_output_bytes: request
                    .limits
                    .max_output_bytes
                    .clamp(1024, MAX_STREAM_BYTES),
                max_items: None,
                concurrency: None,
            },
            record_mode: "never",
        }
    }

    pub fn rejection(request: &Request, problem: Problem) -> Self {
        let mut result = Self::new(request);
        result.reject(problem);
        result
    }

    pub fn reject(&mut self, problem: Problem) {
        self.execution.status = if problem.unsupported {
            Status::Unsupported
        } else {
            Status::Denied
        };
        self.error(problem.code, problem.message);
    }

    pub fn error(&mut self, code: &str, message: &str) {
        self.errors.push(ErrorRecord {
            code: code.into(),
            message: message.into(),
            retryable: false,
        });
    }

    pub fn finish(&mut self, start: Instant) {
        self.finished_at = timestamp();
        self.duration_ms = start.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.bound_output();
    }

    pub fn exit_code(&self) -> u8 {
        match self.data.get("cancellation_signal").and_then(Value::as_u64) {
            Some(2) => return 130,
            Some(15) => return 143,
            _ => {}
        }
        match self.execution.status {
            Status::Succeeded => 0,
            Status::Denied | Status::Unsupported => 2,
            _ => 1,
        }
    }

    fn note_output_loss(&mut self) {
        if self.execution.status == Status::Succeeded {
            self.execution.status = Status::Partial;
        }
        self.coverage.state = "partial".into();
        if !self.errors.iter().any(|e| e.code == "output_truncated") {
            self.error(
                "output_truncated",
                "Output was discarded to enforce a capture or encoded-result limit.",
            );
            self.coverage.limitations.push(
                "Captured output is incomplete because a configured bound was reached.".into(),
            );
        }
    }

    /// Truncate strings, never serialized JSON. Count escaping as well as UTF-8 bytes.
    pub fn bound_output(&mut self) {
        if self.output.stdout_truncated || self.output.stderr_truncated {
            self.note_output_loss();
        }
        loop {
            // Serialization of these explicit primitive response models is infallible.
            let length = serde_json::to_vec(&self)
                .expect("serializable result")
                .len()
                + 1;
            if length <= MAX_RESULT_BYTES {
                break;
            }
            // Grafana's preliminary encoding check precedes final timing and source
            // metadata. Enforce the structured limit on the complete receipt here.
            if matches!(
                self.operation.as_str(),
                "grafana.dashboard.get" | "grafana.query"
            ) && self
                .data
                .get("response")
                .is_some_and(|value| !value.is_null())
            {
                self.data["response"] = Value::Null;
                if self.execution.status == Status::Succeeded {
                    self.execution.status = Status::Failed;
                }
                self.coverage.state = "not_established".into();
                self.error("structured_response_too_large", "The complete encoded Grafana receipt exceeded its limit; the structured response was omitted.");
                continue;
            }
            self.note_output_loss();
            let text = if self.output.stdout.len() >= self.output.stderr.len() {
                &mut self.output.stdout
            } else {
                &mut self.output.stderr
            };
            if text.is_empty() {
                break;
            } // Non-output data is separately bounded by the operation.
            let mut keep = text
                .len()
                .saturating_sub((length - MAX_RESULT_BYTES).max(text.len() / 4));
            while !text.is_char_boundary(keep) {
                keep -= 1;
            }
            if self.output.stdout.len() >= self.output.stderr.len() {
                self.output.stdout.truncate(keep);
                self.output.stdout_truncated = true;
            } else {
                self.output.stderr.truncate(keep);
                self.output.stderr_truncated = true;
            }
        }
    }

    pub fn json_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(self).expect("serializable result");
        bytes.push(b'\n');
        bytes
    }

    pub fn text(&self) -> String {
        let mut text = format!(
            "{}: {}\nRun: {}\nAssessment: {}\nEffects: {}\nRecord: {}\n",
            self.operation,
            serde_json::to_value(&self.execution.status)
                .expect("status")
                .as_str()
                .unwrap_or("unknown"),
            self.run_id,
            self.assessment,
            self.effect_outcome,
            self.record_mode
        );
        if let Some(code) = self.execution.child_exit_code {
            text.push_str(&format!("Child exit: {code}\n"));
        }
        if let Some(signal) = &self.execution.signal {
            text.push_str(&format!("Child signal: {signal}\n"));
        }
        if self.data["profile"] == "linux-read-v1" {
            text.push_str(&format!(
                "Read profile: linux-read-v1; isolation {}\n",
                self.data["isolation"]["state"]
                    .as_str()
                    .unwrap_or("unconfirmed")
            ));
            if self.data["inspection_advisory"] == true {
                text.push_str("Inspection only; no command or namespace probe was started.\n");
            } else if let Some(code) = self.data["tool"]["exit_status"].as_u64() {
                text.push_str(&format!(
                    "Tool exit status: {code} (exit code or 128 + signal)\n"
                ));
            } else {
                text.push_str("Tool execution could not be confirmed from the launcher and wrapper receipts.\n");
            }
        }
        if let Some(cwd) = self.resolved_target.get("cwd").and_then(Value::as_str) {
            text.push_str(&format!("Working directory: {}\n", terminal_safe(cwd)));
        }
        if let Some(exe) = self
            .resolved_target
            .get("executable")
            .and_then(Value::as_str)
        {
            text.push_str(&format!("Executable: {}\n", terminal_safe(exe)));
        }
        for error in &self.errors {
            text.push_str(&format!("{}: {}\n", error.code, error.message));
        }
        if self.operation == "task.run" {
            if let Some(calculation) = self.data.get("calculation") {
                text.push_str(&format!(
                    "Allowed bad fraction: {}\n",
                    display_number(&calculation["allowed_bad_fraction"])
                ));
                if calculation["status"].is_object() {
                    let status = &calculation["status"];
                    let unit = status["unit"].as_str().unwrap_or("unknown");
                    text.push_str(&format!(
                        "Budget: {} {unit}\nConsumed: {} {unit} ({}%)\nRemaining: {} {unit} — {}\n",
                        display_number(&status["budget"]),
                        display_number(&status["consumed"]),
                        display_number(&status["consumed_percent"]),
                        display_number(&status["remaining"]),
                        status["state"].as_str().unwrap_or("unknown")
                    ));
                    if status["observed_availability_percent"].is_number() {
                        text.push_str(&format!(
                            "Supplied request availability: {}%\n",
                            display_number(&status["observed_availability_percent"])
                        ));
                    }
                }
                if calculation["burn"].is_object() {
                    let burn = &calculation["burn"];
                    let short = if burn["short_rate"].is_number() {
                        format!("{}x", display_number(&burn["short_rate"]))
                    } else {
                        "not supplied".into()
                    };
                    let reason = if burn["reason"] == "both_windows_at_threshold" {
                        "both windows at or above threshold".into()
                    } else {
                        burn["reason"]
                            .as_str()
                            .unwrap_or("unknown")
                            .replace('_', " ")
                    };
                    text.push_str(&format!(
                        "Burn ({}): {}x; short ({}): {}\nThreshold: {}x\nSeverity: {} — {}\n",
                        burn["long_window"].as_str().unwrap_or("unknown"),
                        display_number(&burn["long_rate"]),
                        burn["short_window"].as_str().unwrap_or("unknown"),
                        short,
                        display_number(&burn["threshold"]),
                        burn["severity"].as_str().unwrap_or("unknown"),
                        reason
                    ));
                    if burn["full_budget_projection_days"].is_number() {
                        text.push_str(&format!(
                            "Full-budget projection: {} days\n",
                            display_number(&burn["full_budget_projection_days"])
                        ));
                    } else {
                        text.push_str(&format!(
                            "Full-budget projection: {}\n",
                            burn["projection_state"]
                                .as_str()
                                .unwrap_or("unknown")
                                .replace('_', " ")
                        ));
                    }
                }
            }
            if let Some(count) = self.data.get("checked_panels").and_then(Value::as_u64) {
                text.push_str(&format!(
                    "Checked panels: {count}\nFindings: {}\n",
                    self.data["findings_total"]
                ));
                if let Some(findings) = self.data.get("findings").and_then(Value::as_array) {
                    for finding in findings {
                        text.push_str(&format!(
                            "{}: {}\n  {}\n",
                            terminal_safe(finding["rule"].as_str().unwrap_or("unknown")),
                            terminal_safe(finding["location"].as_str().unwrap_or("unknown")),
                            terminal_safe(finding["detail"].as_str().unwrap_or("unknown"))
                        ));
                    }
                }
            }
            text.push_str(&format!(
                "Coverage: {} — {}\n",
                self.coverage.state,
                terminal_safe(&self.coverage.scope)
            ));
            for limitation in &self.coverage.limitations {
                text.push_str(&format!("Limit: {}\n", terminal_safe(limitation)));
            }
        } else if matches!(
            self.operation.as_str(),
            "grafana.dashboard.get" | "grafana.query"
        ) {
            text.push_str(&format!("Connection: {}\n", terminal_safe(&self.target.id)));
            if self.data["binding"].is_object() {
                text.push_str(&format!(
                    "Origin: {}\nOrganization: {} (verified: {})\n",
                    terminal_safe(&self.data["binding"]["origin"].to_string()),
                    self.data["binding"]["organization_id"],
                    self.data["binding"]["organization_verified"]
                ));
            }
            if !self.data["response"].is_null() {
                text.push_str(&format!(
                    "Observation:\n{}\n",
                    terminal_safe(&self.data["response"].to_string())
                ));
            }
            if self.data["redaction"]["applied"] == true {
                text.push_str("Private credential echoes were redacted.\n");
            }
            text.push_str(&format!(
                "Coverage: {} — {}\n",
                self.coverage.state,
                terminal_safe(&self.coverage.scope)
            ));
            for limitation in &self.coverage.limitations {
                text.push_str(&format!("Limit: {}\n", terminal_safe(limitation)));
            }
        } else if self.operation != "process.exec" {
            text.push_str(&format!("{}\n", self.data));
        }
        for (name, output, truncated) in [
            ("stdout", &self.output.stdout, self.output.stdout_truncated),
            ("stderr", &self.output.stderr, self.output.stderr_truncated),
        ] {
            if !output.is_empty() || truncated {
                text.push_str(&format!(
                    "{name}{}:\n{}",
                    if truncated { " (truncated)" } else { "" },
                    terminal_safe(output)
                ));
                if !output.ends_with('\n') {
                    text.push('\n');
                }
            }
        }
        text
    }
}

fn display_number(value: &Value) -> String {
    let Some(value) = value.as_f64() else {
        return "not supplied".into();
    };
    if value == 0.0 {
        return "0".into();
    }
    if value.abs() < 0.000001 || value.abs() >= 1_000_000_000.0 {
        return format!("{value:.6e}");
    }
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn terminal_safe(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

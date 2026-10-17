use crate::request::{
    Limits, MAX_RESULT_BYTES, MAX_STREAM_BYTES, Problem, Request, Target, bounded, valid_operation,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Grafana admits response bodies through depth 64. Retained receipts add the
/// result/data envelope and can add credential-redaction marker objects. Readers
/// share this bounded allowance rather than imposing a tighter response profile.
pub(crate) const MAX_RECEIPT_DEPTH: usize = 72;

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
                "grafana.dashboard.get" | "grafana.query" | "query.run"
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
            if self.operation == "report.create" && self.data["rendering"]["content"].is_string() {
                self.data["rendering"]["content"] = Value::Null;
                self.data["rendering"]["state"] = json!("omitted");
                self.data["report"]["state"] = json!("partial");
                self.execution.status = Status::Partial;
                self.coverage.state = "partial".into();
                let limitation = "The rendered report was omitted because the complete encoded receipt exceeded its limit.";
                self.coverage.limitations.push(limitation.into());
                if let Some(limitations) = self.data["report"]["limitations"].as_array_mut() {
                    limitations.push(json!(limitation));
                }
                self.error("report_rendering_omitted", limitation);
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
        if self.operation.starts_with("change.") {
            text.push_str(&format!("Change action: {}\nState: {}\nRequester: {}\nTarget: {}\nPlan SHA-256: {}\nApproval: {}\nRemote error: {}\n", terminal_safe(&self.data["action"].to_string()), terminal_safe(&self.data["state"].to_string()), terminal_safe(&self.data["requester"].to_string()), terminal_safe(&self.data["target"].to_string()), terminal_safe(&self.data["plan_sha256"].to_string()), terminal_safe(&self.data["approval_id"].to_string()), terminal_safe(&self.data["remote_error"].to_string())));
            if self.data["plan"].is_object() {
                text.push_str(&format!(
                    "Exact plan (untrusted file contents are base64): {}\n",
                    terminal_safe(&self.data["plan"].to_string())
                ));
            }
        }
        if self.operation.starts_with("schedule.") {
            text.push_str(&format!(
                "Schedule action: {}\n",
                terminal_safe(self.data["action"].as_str().unwrap_or("unknown"))
            ));
            if let Some(rows) = self.data["schedules"].as_array() {
                for row in rows {
                    text.push_str(&format!(
                        "{} revision {} generation {} enabled {} occurrences {} active {} timezone {} tzdb {}\n",
                        terminal_safe(row["id"].as_str().unwrap_or("?")),
                        row["revision"],
                        row["generation"],
                        row["enabled"],
                        row["total_occurrences"],
                        row["active_occurrences"],
                        terminal_safe(row["definition"]["timezone"].as_str().unwrap_or("unknown")),
                        terminal_safe(row["tzdb_version"].as_str().unwrap_or("unknown"))
                    ));
                }
            }
            if let Some(rows) = self.data["occurrences"].as_array() {
                for row in rows {
                    text.push_str(&format!(
                        "Occurrence {} {} fleet {} outcome {} cancel_requested {} reason {}\n",
                        terminal_safe(row["id"].as_str().unwrap_or("?")),
                        row["state"],
                        terminal_safe(row["fleet_request_id"].as_str().unwrap_or("?")),
                        row["fleet_aggregate"],
                        row["cancel_requested"],
                        terminal_safe(&row["reason"].to_string())
                    ));
                }
            }
            if let Some(rows) = self.data["preview"].as_array() {
                for row in rows {
                    text.push_str(&format!(
                        "Preview ordinal {} due UTC {} scheduled UTC {} local {} state {}\n",
                        row["ordinal"],
                        terminal_safe(row["due_utc"].as_str().unwrap_or("unknown")),
                        terminal_safe(row["scheduled_utc"].as_str().unwrap_or("none")),
                        terminal_safe(row["local"].as_str().unwrap_or("unknown")),
                        terminal_safe(row["state"].as_str().unwrap_or("unknown"))
                    ));
                }
            }
        }
        if self.operation.starts_with("fleet.") {
            text.push_str(&format!("Fleet run: {}\nAggregate: {}\nCounts: {}\nTargets (untrusted remote observations): {}\n",terminal_safe(&self.data["run"].to_string()),terminal_safe(&self.data["aggregate"].to_string()),terminal_safe(&self.data["counts"].to_string()),terminal_safe(&self.data["targets"].to_string())));
        }
        if self.operation.starts_with("runner.") {
            text.push_str(&format!("Authenticated runner: {}\nPrincipal: {}\nAdmission: {}\nRecovery request ID: {}\nTarget snapshot: {}\n", terminal_safe(&self.data["runner"].to_string()), terminal_safe(&self.data["principal"].to_string()), terminal_safe(&self.data["admission"].to_string()), terminal_safe(&self.data["request_id"].to_string()), terminal_safe(&self.data["target"].to_string())));
        }
        if self.operation.starts_with("job.") || self.operation.starts_with("runner.") {
            text.push_str(&format!(
                "Job action: {}\nJob: {}\nEvents: {}\nCursor: {}\n",
                terminal_safe(self.data["action"].as_str().unwrap_or("not established")),
                terminal_safe(&self.data["job"].to_string()),
                terminal_safe(&self.data["events"].to_string()),
                terminal_safe(&self.data["cursor"].to_string())
            ));
            if self.data["result"].is_object() {
                text.push_str(&format!(
                    "Stored child receipt (untrusted output): {}\n",
                    terminal_safe(&self.data["result"].to_string())
                ));
            }
            if self.data["result_omitted"] == true {
                text.push_str(
                    "Stored result omitted by the detail budget; its digest is retained.\n",
                );
            }
        } else if self.operation.starts_with("ext.") {
            text.push_str("External execution under ordinary operator-reviewed authority; not an untrusted-code sandbox.\n");
            for field in [
                "package",
                "authorization",
                "protocol",
                "progress",
                "terminal",
                "diagnostics",
            ] {
                text.push_str(&format!(
                    "{field}: {}\n",
                    terminal_safe(&self.data[field].to_string())
                ));
            }
        } else if self.operation.starts_with("extension.") {
            text.push_str(&format!("Package action: {}\nInstalled: {}; descriptions: {}; omitted: {}\n{}\n",
                terminal_safe(self.data["action"].as_str().unwrap_or("not established")),
                self.data["installed_count"], self.data["total_packages"], self.data["omitted_packages"],
                if self.data["execution_available"]==true {"Execution authorized for selected exact capabilities by attached policy; provenance unverified."}else{"Disabled; execution unavailable; provenance unverified."}));
            if let Some(packages) = self.data["packages"].as_array() {
                for package in packages {
                    text.push_str(&format!("Package {} version {} — {} ({} bytes)\nManifest (untrusted declarations): {}\n",
                        terminal_safe(package["manifest"]["id"].as_str().unwrap_or("unknown")),
                        package["manifest"]["version"],
                        terminal_safe(package["package_sha256"].as_str().unwrap_or("unknown")),
                        package["package_bytes"], terminal_safe(&package["manifest"].to_string())));
                }
            }
            for limitation in &self.coverage.limitations {
                text.push_str(&format!("Limit: {}\n", terminal_safe(limitation)));
            }
        } else if self.operation == "report.create" {
            text.push_str(&format!(
                "Coverage: {} — {}\n",
                self.coverage.state,
                terminal_safe(&self.coverage.scope)
            ));
            for limitation in &self.coverage.limitations {
                text.push_str(&format!("Limit: {}\n", terminal_safe(limitation)));
            }
            if let Some(document) = self.data["rendering"]["content"].as_str() {
                text.push('\n');
                text.push_str(&terminal_safe(document));
            } else if self.data["rendering"]["state"] == "omitted" {
                text.push_str("Rendered report omitted by the output budget; structured report is available with --json.\n");
            }
        } else if matches!(self.operation.as_str(), "task.run" | "config.check") {
            if self.operation == "config.check" && self.data["validator"].is_object() {
                text.push_str(&format!("Configuration kind: {}\nValidator: {}\nInput: {}\nValidation: {}\nUnsupported sections: {}\n",
                    terminal_safe(self.data["kind"].as_str().unwrap_or("unknown")),
                    terminal_safe(&self.data["validator"].to_string()),
                    terminal_safe(&self.data["input"].to_string()),
                    self.data["validation_state"].as_str().unwrap_or("not_established"),
                    terminal_safe(&self.data["unsupported_sections"].to_string())));
            }
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
            if self
                .data
                .get("findings_total")
                .and_then(Value::as_u64)
                .is_some()
            {
                if let Some(count) = self.data.get("checked_panels").and_then(Value::as_u64) {
                    text.push_str(&format!("Checked panels: {count}\n"));
                }
                if self.data.get("checked_rules").is_some() {
                    text.push_str(&format!(
                        "Checked rules: {}\nOutcome: {}\n",
                        self.data["checked_rules"]
                            .as_u64()
                            .map(|count| count.to_string())
                            .unwrap_or_else(|| "not established".into()),
                        self.data["validation_outcome"]
                            .as_str()
                            .unwrap_or("unknown")
                    ));
                }
                text.push_str(&format!("Findings: {}\n", self.data["findings_total"]));
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
            if let Some(diagnostics) = self.data.get("diagnostics") {
                for stream in ["stdout", "stderr"] {
                    if let Some(value) = diagnostics[stream]
                        .as_str()
                        .filter(|value| !value.is_empty())
                    {
                        text.push_str(&format!(
                            "Native {stream} (untrusted):\n{}\n",
                            terminal_safe(value)
                        ));
                    }
                }
                if diagnostics["truncated"] == true {
                    text.push_str("Native diagnostics are incomplete.\n");
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
            "grafana.dashboard.get" | "grafana.query" | "query.run"
        ) {
            text.push_str(&format!("Connection: {}\n", terminal_safe(&self.target.id)));
            if self.operation == "query.run" && self.data["definition"].is_object() {
                text.push_str(&format!(
                    "Definition: {}\nParameters: {}\nWindow: {}\n",
                    terminal_safe(&self.data["definition"].to_string()),
                    terminal_safe(&self.data["parameters"].to_string()),
                    terminal_safe(&self.data["window"].to_string())
                ));
            }
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
        } else if self.operation == "diagnostics.run"
            || self.operation.starts_with("evidence.")
            || self.operation.starts_with("runbook.")
        {
            text.push_str(&format!(
                "Connection: {}\nObservation:\n{}\nCoverage: {} — {}\n",
                terminal_safe(&self.target.id),
                terminal_safe(&self.data.to_string()),
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

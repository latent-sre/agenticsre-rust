use rmcp::model::{
    ErrorCode, ErrorData, JsonRpcError, JsonRpcResponse, JsonRpcVersion2_0, NumberOrString,
    ProtocolVersion, RequestId, RequestMetaObject,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{self, Write};

pub(crate) const MAX_INPUT: usize = 128 * 1024;
pub(crate) const MAX_OUTPUT: usize = 3 * 1024 * 1024;
pub(crate) const MODERN: &str = "2026-07-28";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Id {
    String(String),
    Number(i64),
}
impl Id {
    pub fn parse(value: &Value) -> Option<Self> {
        match value {
            Value::String(value) if value.len() <= 128 => Some(Self::String(value.clone())),
            Value::Number(value) => value
                .as_i64()
                .filter(|v| (-9_007_199_254_740_991..=9_007_199_254_740_991).contains(v))
                .map(Self::Number),
            _ => None,
        }
    }
    fn wire(&self) -> RequestId {
        match self {
            Self::String(v) => NumberOrString::String(v.clone().into()),
            Self::Number(v) => NumberOrString::Number(*v),
        }
    }
}

pub(crate) struct CappedWriter(Vec<u8>);
impl Write for CappedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_OUTPUT - self.0.len() {
            return Err(io::Error::other("outbound frame limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn serialize(value: &impl Serialize) -> Result<Vec<u8>, ()> {
    let mut writer = CappedWriter(Vec::new());
    serde_json::to_writer(&mut writer, value).map_err(|_| ())?;
    writer.write_all(b"\n").map_err(|_| ())?;
    Ok(writer.0)
}
pub(crate) fn reply(id: &Id, result: &impl Serialize) -> Result<Vec<u8>, ()> {
    serialize(&JsonRpcResponse {
        jsonrpc: JsonRpcVersion2_0,
        id: id.wire(),
        result,
    })
}
pub(crate) fn error(
    id: Option<&Id>,
    code: ErrorCode,
    message: &'static str,
) -> Result<Vec<u8>, ()> {
    serialize(&JsonRpcError::new(
        id.map(Id::wire),
        ErrorData::new(code, message, None),
    ))
}
pub(crate) fn unsupported(id: &Id, requested: ProtocolVersion) -> Result<Vec<u8>, ()> {
    serialize(&JsonRpcError::new(
        Some(id.wire()),
        ErrorData::unsupported_protocol_version(
            requested,
            &[ProtocolVersion::V_2026_07_28, ProtocolVersion::V_2025_11_25],
        ),
    ))
}

pub(crate) fn modern(params: &Value) -> Result<bool, Option<ProtocolVersion>> {
    let Some(meta) = params.get("_meta") else {
        return Ok(false);
    };
    if !meta.is_object() {
        return Err(None);
    }
    let typed: RequestMetaObject = serde_json::from_value(meta.clone()).map_err(|_| None)?;
    if meta
        .get("io.modelcontextprotocol/protocolVersion")
        .is_none()
    {
        return Ok(false);
    }
    let version = typed.protocol_version().ok_or(None)?;
    if version.as_str() != MODERN {
        return Err(Some(version));
    }
    if !meta["io.modelcontextprotocol/clientCapabilities"].is_object()
        || meta
            .get("io.modelcontextprotocol/clientInfo")
            .is_some_and(|info| {
                !info.is_object()
                    || serde_json::from_value::<rmcp::model::Implementation>(info.clone()).is_err()
            })
        || !typed.missing_required_keys(&version).is_empty()
    {
        return Err(None);
    }
    Ok(true)
}

pub(crate) fn receipt(
    id: &Id,
    modern: bool,
    result: &workbench_core::result::OperationResult,
) -> Result<Vec<u8>, ()> {
    use workbench_core::result::Status;
    // A fixed summary is independent of arbitrary command output, target content and credentials.
    let status = match result.execution.status {
        Status::Succeeded => "succeeded",
        Status::Failed => "failed",
        Status::Partial => "partial",
        Status::Denied => "denied",
        Status::Unsupported => "unsupported",
        Status::Cancelled => "cancelled",
        Status::TimedOut => "timed_out",
        Status::Unknown => "unknown",
    };
    let incomplete = result.output.stdout_truncated
        || result.output.stderr_truncated
        || result.coverage.state == "partial";
    let summary = format!(
        "Workbench execution {status}; assessment {}. Inspect structuredContent for the complete receipt.",
        result.assessment
    );
    // Borrow the receipt directly: no unbounded Value conversion or duplicate receipt text.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ToolResult<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        result_type: Option<rmcp::model::ResultType>,
        content: [Value; 1],
        structured_content: &'a workbench_core::result::OperationResult,
        is_error: bool,
    }
    reply(
        id,
        &ToolResult {
            result_type: modern.then_some(rmcp::model::ResultType::COMPLETE),
            content: [json!({"type":"text","text":summary})],
            structured_content: result,
            is_error: result.execution.status != Status::Succeeded || incomplete,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use workbench_core::{
        request::Request,
        result::{OperationResult, Status},
    };
    #[test]
    fn output_limit_includes_json_escaping_and_delimiter() {
        assert!(serialize(&"a".repeat(MAX_OUTPUT - 3)).is_ok());
        assert!(serialize(&"a".repeat(MAX_OUTPUT - 2)).is_err());
        assert!(serialize(&"\0".repeat(MAX_OUTPUT / 6)).is_err());
    }
    #[test]
    fn tool_errors_derive_from_execution_and_capture_not_budget_findings() {
        let request = Request::new(
            "task.run",
            json!({"id":"error-budget","version":1,"input":{"slo":99.9,"bad_minutes":1000}}),
        );
        let core = workbench_core::execute(request, &RunControlForTest::default());
        let result: Value =
            serde_json::from_slice(&receipt(&Id::Number(1), true, &core).unwrap()).unwrap();
        assert_eq!(result["result"]["isError"], false);
        assert_eq!(
            result["result"]["structuredContent"],
            serde_json::to_value(&core).unwrap()
        );
        let mut core = OperationResult::new(&Request::new("command.inspect", json!({})));
        for status in [
            Status::Failed,
            Status::Partial,
            Status::Denied,
            Status::Unsupported,
            Status::Cancelled,
            Status::TimedOut,
            Status::Unknown,
        ] {
            core.execution.status = status;
            let result: Value =
                serde_json::from_slice(&receipt(&Id::String("id".into()), false, &core).unwrap())
                    .unwrap();
            assert_eq!(result["result"]["isError"], true);
            assert!(result["result"].get("resultType").is_none());
        }
        core.execution.status = Status::Succeeded;
        core.output.stdout_truncated = true;
        let result: Value =
            serde_json::from_slice(&receipt(&Id::Number(1), true, &core).unwrap()).unwrap();
        assert_eq!(result["result"]["isError"], true);
    }
    use workbench_core::RunControl as RunControlForTest;
}

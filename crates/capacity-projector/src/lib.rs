//! A separate offline extension consumer, with no dependency on the workbench host.
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const OPERATION: &str = "ext.example.capacity.project";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub protocol: u64,
    pub request_id: String,
    pub run_id: String,
    pub sequence: u64,
    pub kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: u64,
    pub request_id: String,
    pub run_id: String,
    pub sequence: u64,
    pub kind: String,
    pub operation: String,
    pub operation_version: u64,
    pub inputs: Inputs,
    pub limits: Limits,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub growth_bytes_per_day: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Projection {
    pub headroom_bytes: u64,
    pub days_remaining_floor: u64,
    pub exhausted: bool,
    pub has_finite_horizon: bool,
    pub projection_state: &'static str,
    pub assumption: &'static str,
}

#[derive(Serialize)]
struct Progress<'a> {
    protocol: u64,
    request_id: &'a str,
    run_id: &'a str,
    sequence: u64,
    kind: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct Terminal<'a> {
    protocol: u64,
    request_id: &'a str,
    run_id: &'a str,
    sequence: u64,
    kind: &'static str,
    status: &'static str,
    data: Option<Projection>,
    error: Option<PeerError>,
}

#[derive(Serialize)]
struct PeerError {
    code: &'static str,
    message: &'static str,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The byte limit includes the terminating LF. Read at most one extra byte to
/// distinguish an exactly bounded frame from overflow without unbounded input.
pub fn read_frame<R: BufRead, T: serde::de::DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let mut frame = Vec::new();
    reader
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut frame)?;
    if frame.len() > MAX_FRAME_BYTES || frame.last() != Some(&b'\n') {
        return Err(invalid("missing LF or oversized protocol frame"));
    }
    // Serde's derived struct visitors also accept positional arrays. The wire
    // protocol requires objects, including its two nested request records.
    let shape: serde_json::Value =
        serde_json::from_slice(&frame).map_err(|_| invalid("invalid protocol JSON"))?;
    let object = shape
        .as_object()
        .ok_or_else(|| invalid("protocol frame must be an object"))?;
    for field in ["inputs", "limits"] {
        if object.get(field).is_some_and(|value| !value.is_object()) {
            return Err(invalid("protocol request records must be objects"));
        }
    }
    // Deserialize the original bytes, not the Value: derived visitors must
    // still see and reject duplicate control and input keys.
    serde_json::from_slice(&frame).map_err(|_| invalid("invalid protocol JSON"))
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let frame =
        serde_json::to_vec(value).map_err(|_| invalid("cannot serialize protocol frame"))?;
    if frame.len() >= MAX_FRAME_BYTES {
        return Err(invalid("oversized response frame"));
    }
    writer.write_all(&frame)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

pub fn validate_hello(hello: &Hello) -> io::Result<()> {
    let identifier =
        |value: &str| !value.is_empty() && value.chars().count() <= 128 && !value.contains('\0');
    if hello.protocol != 1
        || hello.sequence != 0
        || hello.kind != "hello"
        || !identifier(&hello.request_id)
        || !identifier(&hello.run_id)
    {
        return Err(invalid("unsupported or invalid host hello"));
    }
    Ok(())
}

pub fn validate_request(request: &Request, hello: &Hello) -> io::Result<()> {
    if request.protocol != 1
        || request.request_id != hello.request_id
        || request.run_id != hello.run_id
        || request.sequence != 1
        || request.kind != "request"
    {
        return Err(invalid("inconsistent request correlation or sequence"));
    }
    // The host may supply a durable job runtime of up to one hour;
    // ordinary foreground admission keeps its separate 300-second ceiling.
    if !(1..=3_600_000).contains(&request.limits.timeout_ms)
        || !(1024..=1_048_576).contains(&request.limits.max_output_bytes)
    {
        return Err(invalid("unsupported request limits"));
    }
    Ok(())
}

pub fn project(inputs: &Inputs) -> io::Result<Projection> {
    if [
        inputs.capacity_bytes,
        inputs.used_bytes,
        inputs.growth_bytes_per_day,
    ]
    .into_iter()
    .any(|value| value > MAX_SAFE_INTEGER)
    {
        return Err(invalid("capacity quantities exceed safe integer range"));
    }
    let headroom_bytes = inputs.capacity_bytes.saturating_sub(inputs.used_bytes);
    let exhausted = inputs.used_bytes >= inputs.capacity_bytes;
    let (days_remaining_floor, has_finite_horizon, projection_state) = if exhausted {
        (0, true, "already_full")
    } else if let Some(days) = headroom_bytes.checked_div(inputs.growth_bytes_per_day) {
        (days, true, "growing")
    } else {
        (0, false, "not_growing")
    };
    Ok(Projection {
        headroom_bytes,
        days_remaining_floor,
        exhausted,
        has_finite_horizon,
        projection_state,
        assumption: "Constant supplied daily growth; no live measurements.",
    })
}

/// Serve one invocation. Malformed control frames have no terminal success;
/// understood unsupported operations and invalid quantities receive failure.
pub fn serve<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<()> {
    let hello: Hello = read_frame(reader)?;
    validate_hello(&hello)?;
    write_frame(writer, &hello)?;
    let request: Request = read_frame(reader)?;
    validate_request(&request, &hello)?;
    write_frame(
        writer,
        &Progress {
            protocol: 1,
            request_id: &hello.request_id,
            run_id: &hello.run_id,
            sequence: 1,
            kind: "progress",
            message: "Checking supplied capacity projection inputs.",
        },
    )?;
    let outcome = if request.operation != OPERATION || request.operation_version != 1 {
        Err(PeerError {
            code: "unsupported_operation",
            message: "Only capacity projection operation version 1 is supported.",
        })
    } else {
        project(&request.inputs).map_err(|_| PeerError {
            code: "invalid_input",
            message: "Capacity quantities must be integers from zero through 9007199254740991.",
        })
    };
    let (status, data, error) = match outcome {
        Ok(data) => ("succeeded", Some(data), None),
        Err(error) => ("failed", None, Some(error)),
    };
    write_frame(
        writer,
        &Terminal {
            protocol: 1,
            request_id: &hello.request_id,
            run_id: &hello.run_id,
            sequence: 2,
            kind: "result",
            status,
            data,
            error,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::io::Cursor;

    #[test]
    fn projection_uses_integer_floor_and_discloses_nonfinite_and_exhausted_cases() {
        let calculate = |capacity_bytes, used_bytes, growth_bytes_per_day| {
            project(&Inputs {
                capacity_bytes,
                used_bytes,
                growth_bytes_per_day,
            })
            .unwrap()
        };
        let ordinary = calculate(1_000, 101, 100);
        assert_eq!(
            (ordinary.headroom_bytes, ordinary.days_remaining_floor),
            (899, 8)
        );
        assert_eq!(ordinary.projection_state, "growing");
        let subday = calculate(100, 99, 2);
        assert_eq!(
            (
                subday.days_remaining_floor,
                subday.exhausted,
                subday.has_finite_horizon
            ),
            (0, false, true)
        );
        let no_growth = calculate(100, 1, 0);
        assert_eq!(
            (
                no_growth.days_remaining_floor,
                no_growth.has_finite_horizon,
                no_growth.projection_state
            ),
            (0, false, "not_growing")
        );
        for (capacity, used) in [(0, 0), (100, 100), (100, 101)] {
            let full = calculate(capacity, used, 0);
            assert_eq!(
                (
                    full.headroom_bytes,
                    full.days_remaining_floor,
                    full.exhausted,
                    full.has_finite_horizon,
                    full.projection_state
                ),
                (0, 0, true, true, "already_full")
            );
        }
        let maximum = calculate(MAX_SAFE_INTEGER, 0, 1);
        assert_eq!(maximum.days_remaining_floor, 9_007_199_254_740_991);
        assert!(
            project(&Inputs {
                capacity_bytes: MAX_SAFE_INTEGER + 1,
                used_bytes: 0,
                growth_bytes_per_day: 1
            })
            .is_err()
        );
        assert!(
            project(&Inputs {
                capacity_bytes: 1,
                used_bytes: MAX_SAFE_INTEGER + 1,
                growth_bytes_per_day: 1
            })
            .is_err()
        );
        assert!(
            project(&Inputs {
                capacity_bytes: 1,
                used_bytes: 0,
                growth_bytes_per_day: MAX_SAFE_INTEGER + 1
            })
            .is_err()
        );
    }

    fn hello() -> Value {
        json!({"protocol":1,"request_id":"test","run_id":"run","sequence":0,"kind":"hello"})
    }

    fn request() -> Value {
        json!({"protocol":1,"request_id":"test","run_id":"run","sequence":1,"kind":"request","operation":OPERATION,"operation_version":1,"inputs":{"capacity_bytes":1000,"used_bytes":100,"growth_bytes_per_day":30},"limits":{"timeout_ms":1000,"max_output_bytes":4096}})
    }

    fn transcript(hello: &Value, request: &Value) -> (io::Result<()>, Vec<Value>) {
        let input = format!("{hello}\n{request}\n");
        let mut output = Vec::new();
        let result = serve(&mut Cursor::new(input), &mut output);
        let frames = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        (result, frames)
    }

    #[test]
    fn transcript_calculates_and_correlates_exactly_one_terminal() {
        let (result, frames) = transcript(&hello(), &request());
        result.unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0], hello());
        assert_eq!(frames[1]["sequence"], 1);
        assert_eq!(frames[1]["kind"], "progress");
        assert_eq!(frames[2]["sequence"], 2);
        assert_eq!(frames[2]["kind"], "result");
        assert_eq!(frames[2]["status"], "succeeded");
        assert_eq!(frames[2]["data"]["days_remaining_floor"], 30);
        assert_eq!(frames[2]["data"]["headroom_bytes"], 900);
        assert_eq!(frames[2]["error"], Value::Null);
        for frame in frames {
            assert_eq!(frame["request_id"], "test");
            assert_eq!(frame["run_id"], "run");
            assert_eq!(frame["protocol"], 1);
        }
    }

    #[test]
    fn unsupported_versions_and_inconsistent_controls_cannot_succeed() {
        let mut old = hello();
        old["protocol"] = json!(0);
        let (result, frames) = transcript(&old, &request());
        assert!(result.is_err());
        assert!(frames.is_empty());
        let mut old_operation = request();
        old_operation["operation_version"] = json!(0);
        let (result, frames) = transcript(&hello(), &old_operation);
        result.unwrap();
        assert_eq!(frames[2]["status"], "failed");
        assert_eq!(frames[2]["error"]["code"], "unsupported_operation");
        assert_eq!(frames[2]["data"], Value::Null);
        for field in ["run_id", "request_id", "sequence", "protocol", "kind"] {
            let mut inconsistent = request();
            inconsistent[field] = if field == "sequence" || field == "protocol" {
                json!(2)
            } else {
                json!("wrong")
            };
            let (result, frames) = transcript(&hello(), &inconsistent);
            assert!(result.is_err(), "{field}");
            assert_eq!(frames, vec![hello()]);
        }
    }

    #[test]
    fn durable_job_runtime_accepts_one_hour_and_rejects_out_of_range_limits() {
        let mut job = request();
        job["limits"]["timeout_ms"] = json!(3_600_000);
        let (result, frames) = transcript(&hello(), &job);
        result.unwrap();
        assert_eq!(frames[2]["status"], "succeeded");
        assert_eq!(frames[2]["data"]["days_remaining_floor"], 30);
        for timeout in [0, 3_600_001] {
            job["limits"]["timeout_ms"] = json!(timeout);
            let (result, frames) = transcript(&hello(), &job);
            assert!(result.is_err(), "timeout {timeout}");
            assert_eq!(frames, vec![hello()]);
        }
    }

    #[test]
    fn missing_unknown_duplicate_and_noninteger_fields_are_rejected() {
        let mut invalid_requests = Vec::new();
        let mut missing = request();
        missing.as_object_mut().unwrap().remove("limits");
        invalid_requests.push(missing.to_string());
        let mut unknown = request();
        unknown["argv"] = json!([]);
        invalid_requests.push(unknown.to_string());
        for number in [json!(-1), json!(1.5), json!("1")] {
            let mut invalid = request();
            invalid["inputs"]["capacity_bytes"] = number;
            invalid_requests.push(invalid.to_string());
        }
        invalid_requests.push(request().to_string().replacen("{", "{\"protocol\":1,", 1));
        let mut duplicate_input = request().to_string();
        duplicate_input = duplicate_input.replace(
            "\"capacity_bytes\":1000",
            "\"capacity_bytes\":1000,\"capacity_bytes\":1000",
        );
        invalid_requests.push(duplicate_input);
        for request in invalid_requests {
            let input = format!("{}\n{request}\n", hello());
            let mut output = Vec::new();
            assert!(serve(&mut Cursor::new(input), &mut output).is_err());
            assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
        }
    }

    #[test]
    fn positional_arrays_cannot_substitute_for_protocol_objects() {
        let positional_hello = json!([1, "test", "run", 0, "hello"]);
        let (result, frames) = transcript(&positional_hello, &request());
        assert!(result.is_err());
        assert!(frames.is_empty());

        let positional_request = json!([
            1, "test", "run", 1, "request", OPERATION, 1,
            {"capacity_bytes":1000,"used_bytes":100,"growth_bytes_per_day":30},
            {"timeout_ms":1000,"max_output_bytes":4096}
        ]);
        let (result, frames) = transcript(&hello(), &positional_request);
        assert!(result.is_err());
        assert_eq!(frames, vec![hello()]);

        for (field, array) in [
            ("inputs", json!([1000, 100, 30])),
            ("limits", json!([1000, 4096])),
        ] {
            let mut positional = request();
            positional[field] = array;
            let (result, frames) = transcript(&hello(), &positional);
            assert!(result.is_err(), "{field}");
            assert_eq!(frames, vec![hello()]);
        }
    }

    #[test]
    fn frames_require_utf8_and_lf_and_have_a_byte_bound() {
        for input in [
            b"{}".to_vec(),
            vec![0xff, b'\n'],
            vec![b' '; MAX_FRAME_BYTES + 1],
        ] {
            assert!(read_frame::<_, Value>(&mut Cursor::new(input)).is_err());
        }
        let mut exact = vec![b' '; MAX_FRAME_BYTES - 3];
        exact.extend_from_slice(b"{}\n");
        assert_eq!(
            read_frame::<_, Value>(&mut Cursor::new(exact)).unwrap(),
            json!({})
        );
    }
}

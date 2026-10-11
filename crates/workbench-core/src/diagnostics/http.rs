//! Deliberately small HTTP/1 receive profile. RFC 9112 framing is part of admission.
use super::{Outcome, config::Prepared, tls_connect};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const MAX_HEADERS: usize = 8192;
const MAX_FIELDS: usize = 64;

struct Headers {
    status: u16,
    length: Option<u64>,
    chunked: bool,
    encoding: String,
    selected: BTreeMap<String, String>,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn headers(bytes: &[u8]) -> Result<Headers, &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "http_headers_invalid")?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or("http_headers_invalid")?;
    let mut status = status_line.splitn(3, ' ');
    let version = status.next().ok_or("http_headers_invalid")?;
    let code = status.next().ok_or("http_headers_invalid")?;
    let reason = status.next().ok_or("http_headers_invalid")?;
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0")
        || code.len() != 3
        || !code.bytes().all(|b| b.is_ascii_digit())
        || !reason
            .bytes()
            .all(|b| b == b'\t' || (32..=126).contains(&b))
    {
        return Err("http_headers_invalid");
    }
    let status = code.parse::<u16>().map_err(|_| "http_headers_invalid")?;
    if !(100..=599).contains(&status) {
        return Err("http_headers_invalid");
    }
    // No upgrade, interim-response stream or protocol switch is implemented by this profile.
    if status < 200 {
        return Err("http_informational_unsupported");
    }
    let mut fields = BTreeMap::<String, Vec<String>>::new();
    let mut count = 0;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        count += 1;
        if count > MAX_FIELDS {
            return Err("http_headers_too_large");
        }
        let (name, value) = line.split_once(':').ok_or("http_headers_invalid")?;
        if !token(name) || !value.bytes().all(|b| b == b'\t' || (32..=126).contains(&b)) {
            return Err("http_headers_invalid");
        }
        let value = value.trim_matches([' ', '\t']);
        fields
            .entry(name.to_ascii_lowercase())
            .or_default()
            .push(value.to_owned());
    }
    for name in [
        "content-length",
        "transfer-encoding",
        "content-encoding",
        "content-type",
    ] {
        if fields.get(name).is_some_and(|values| values.len() != 1) {
            return Err("http_framing_invalid");
        }
    }
    let length = fields
        .get("content-length")
        .map(|values| {
            let raw = &values[0];
            if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err("http_framing_invalid");
            }
            raw.parse::<u64>().map_err(|_| "http_framing_invalid")
        })
        .transpose()?;
    let chunked = if let Some(values) = fields.get("transfer-encoding") {
        if length.is_some() || version != "HTTP/1.1" || !values[0].eq_ignore_ascii_case("chunked") {
            return Err("http_framing_invalid");
        }
        true
    } else {
        false
    };
    if status == 204 && (length.is_some() || chunked)
        || status == 205 && (length.is_some_and(|n| n != 0) || chunked)
    {
        return Err("http_framing_invalid");
    }
    let encoding = fields
        .get("content-encoding")
        .map(|values| values[0].to_ascii_lowercase())
        .unwrap_or_else(|| "identity".into());
    if !matches!(
        encoding.as_str(),
        "identity" | "gzip" | "deflate" | "br" | "zstd"
    ) {
        return Err("http_encoding_unsupported");
    }
    let selected = ["content-length", "content-type"]
        .into_iter()
        .filter_map(|name| {
            fields
                .get(name)
                .map(|values| (name.to_owned(), values[0].clone()))
        })
        .collect::<BTreeMap<_, _>>();
    if selected.values().any(|value| value.len() > 256) {
        return Err("http_headers_too_large");
    }
    Ok(Headers {
        status,
        length,
        chunked,
        encoding,
        selected,
    })
}

async fn line(
    stream: &mut (impl AsyncRead + Unpin),
    total: &mut usize,
    maximum: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    loop {
        if *total >= maximum || bytes.len() >= 1024 {
            return Err("http_headers_too_large");
        }
        let byte = stream
            .read_u8()
            .await
            .map_err(|_| "http_response_incomplete")?;
        *total += 1;
        bytes.push(byte);
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len() - 2);
            return Ok(bytes);
        }
        if byte == b'\n' {
            return Err("http_headers_invalid");
        }
    }
}

async fn response(
    stream: &mut (impl AsyncRead + Unpin),
    capture: bool,
    maximum: usize,
) -> Result<(Value, bool, bool), &'static str> {
    let mut total = 0;
    let mut bytes = Vec::new();
    loop {
        let next = line(stream, &mut total, MAX_HEADERS).await?;
        bytes.extend_from_slice(&next);
        bytes.extend_from_slice(b"\r\n");
        if next.is_empty() {
            break;
        }
    }
    let headers = headers(&bytes)?;
    let mut body = Vec::new();
    let mut truncated = false;
    let bodyless = matches!(headers.status, 204 | 304);
    if !bodyless && headers.chunked {
        let mut chunks = 0;
        loop {
            let size = line(stream, &mut total, MAX_HEADERS).await?;
            chunks += 1;
            if chunks > 256 {
                return Err("http_chunk_limit_exceeded");
            }
            if size.is_empty() || size.len() > 16 || !size.iter().all(u8::is_ascii_hexdigit) {
                return Err("http_chunk_syntax_unsupported");
            }
            let size = u64::from_str_radix(
                std::str::from_utf8(&size).map_err(|_| "http_framing_invalid")?,
                16,
            )
            .map_err(|_| "http_framing_invalid")?;
            if size == 0 {
                // Trailers are not returned or interpreted. Nonempty trailers are refused rather than guessed.
                if !line(stream, &mut total, MAX_HEADERS).await?.is_empty() {
                    return Err("http_trailers_unsupported");
                }
                break;
            }
            let keep = size.min((maximum + 1 - body.len()) as u64) as usize;
            let original = body.len();
            body.resize(original + keep, 0);
            stream
                .read_exact(&mut body[original..])
                .await
                .map_err(|_| "http_response_incomplete")?;
            if body.len() > maximum || keep as u64 != size {
                truncated = true;
                break;
            }
            let mut terminator = [0_u8; 2];
            if total > MAX_HEADERS - terminator.len() {
                return Err("http_headers_too_large");
            }
            stream
                .read_exact(&mut terminator)
                .await
                .map_err(|_| "http_response_incomplete")?;
            total += terminator.len();
            if terminator != *b"\r\n" {
                return Err("http_framing_invalid");
            }
        }
    } else if !bodyless {
        if let Some(length) = headers.length {
            let keep = length.min((maximum + 1) as u64) as usize;
            body.resize(keep, 0);
            stream
                .read_exact(&mut body)
                .await
                .map_err(|_| "http_response_incomplete")?;
            truncated = length > maximum as u64;
        } else {
            // A close-delimited body is complete only on a clean TLS close-notify. Rustls rejects
            // a transport EOF without it, so an interrupted response cannot become successful.
            let mut buffer = [0_u8; 1024];
            loop {
                let allowance = (maximum + 1 - body.len()).min(buffer.len());
                let count = stream
                    .read(&mut buffer[..allowance])
                    .await
                    .map_err(|_| "http_response_incomplete")?;
                if count == 0 {
                    break;
                }
                body.extend_from_slice(&buffer[..count]);
                if body.len() > maximum {
                    truncated = true;
                    break;
                }
            }
        }
    }
    body.truncate(maximum);
    let status_success = (200..300).contains(&headers.status);
    let observation = json!({"status":headers.status,"headers":headers.selected,"headers_untrusted":true,
        "body_capture":{"requested":capture,"encoding":"base64","content_encoding":headers.encoding,
            "decompressed":false,"limit_bytes":maximum,"captured_bytes":if capture {body.len()} else {0},
            "truncated":truncated,"framing_complete":!truncated,"classification":"untrusted_content",
            "redaction_applied":false,"content":if capture {Some(STANDARD.encode(&body))} else {None}},
        "redirect_followed":false,"proxy_used":false,"retries":0});
    Ok((observation, status_success, truncated))
}

pub(super) async fn observe(
    prepared: &Prepared,
    peer: SocketAddr,
    capture: bool,
    maximum: usize,
) -> Outcome {
    let (mut stream, connection) = match tls_connect(prepared, peer).await {
        Ok(value) => value,
        Err(code) => return Outcome::failed(code, json!({})),
    };
    let host = if prepared.target.host.contains(':') {
        format!("[{}]", prepared.target.host)
    } else {
        prepared.target.host.clone()
    };
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nAccept: */*\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        prepared.target.path, host, prepared.target.port
    );
    if stream.write_all(request.as_bytes()).await.is_err() {
        return Outcome::failed("http_request_failed", connection);
    }
    match response(&mut stream, capture, maximum).await {
        Ok((mut data, success, truncated)) => {
            data["connection"] = connection;
            data["path"] = json!(prepared.target.path);
            if !success {
                Outcome::failed("http_status_failed", data)
            } else if truncated {
                Outcome::partial("http_body_truncated", data)
            } else {
                Outcome::succeeded(data)
            }
        }
        Err(code) => Outcome::failed(code, connection),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn padded_head(initial: &str, size: usize) -> Vec<u8> {
        let mut head = initial.as_bytes().to_vec();
        let mut index = 0;
        while head.len() + 2 < size {
            let field = format!("X{index}: ");
            let available = size - head.len() - 2 - field.len() - 2;
            // Leave room for a whole next field, or put the remaining padding in this line.
            let count = if available <= 916 { available } else { 900 };
            head.extend_from_slice(field.as_bytes());
            head.extend(std::iter::repeat_n(b'x', count));
            head.extend_from_slice(b"\r\n");
            index += 1;
        }
        head.extend_from_slice(b"\r\n");
        assert_eq!(head.len(), size);
        head
    }

    #[test]
    fn aggregate_head_and_all_chunk_crlfs_obey_exact_ceiling() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for (head_size, expected) in [
            (6912, None),
            (6913, Some("http_headers_too_large")),
            (7422, Some("http_headers_too_large")),
        ] {
            let mut input = padded_head(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n",
                head_size,
            );
            for _ in 0..255 {
                input.extend_from_slice(b"1\r\nx\r\n");
            }
            input.extend_from_slice(b"0\r\n\r\n");
            let actual = runtime.block_on(response(&mut input.as_slice(), false, 16384));
            if let Some(expected) = expected {
                assert_eq!(actual.unwrap_err(), expected);
            } else {
                let (observation, success, truncated) = actual.unwrap();
                assert!(success && !truncated);
                assert_eq!(observation["body_capture"]["framing_complete"], true);
            }
        }
        for (head_size, succeeds) in [(8192, true), (8193, false)] {
            let input = padded_head("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n", head_size);
            let actual = runtime.block_on(response(&mut input.as_slice(), false, 16384));
            if succeeds {
                assert!(actual.unwrap().1);
            } else {
                assert_eq!(actual.unwrap_err(), "http_headers_too_large");
            }
        }
    }

    #[test]
    fn bounded_http_profile_refuses_ambiguous_or_unsupported_framing() {
        let cases = [
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
                "http_framing_invalid",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Length: 2, 2\r\n\r\n",
                "http_framing_invalid",
            ),
            (
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n",
                "http_framing_invalid",
            ),
            (
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
                "http_framing_invalid",
            ),
            (
                "HTTP/1.1 100 Continue\r\n\r\n",
                "http_informational_unsupported",
            ),
            (
                "HTTP/1.1 200 OK\r\n folded: field\r\n\r\n",
                "http_headers_invalid",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Encoding: unknown\r\n\r\n",
                "http_encoding_unsupported",
            ),
        ];
        for (input, code) in cases {
            assert!(
                matches!(headers(input.as_bytes()), Err(actual) if actual == code),
                "{input}"
            );
        }
    }
}

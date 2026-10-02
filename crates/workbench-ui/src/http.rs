use crate::App;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, Response, body::Incoming, header};
use serde_json::{Value, json};
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

type Reply = Response<Full<Bytes>>;

pub(crate) struct ApiError {
    pub status: u16,
    code: &'static str,
    title: &'static str,
    detail: &'static str,
    field: Option<&'static str>,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, title: &'static str, detail: &'static str) -> Self {
        Self {
            status,
            code,
            title,
            detail,
            field: None,
        }
    }
    pub fn invalid(field: &'static str, detail: &'static str) -> Self {
        Self {
            status: 422,
            code: "invalid-input",
            title: "Invalid input",
            detail,
            field: Some(field),
        }
    }
    pub fn forbidden(code: &'static str, detail: &'static str) -> Self {
        Self::new(403, code, "Request refused", detail)
    }
    pub fn not_found() -> Self {
        Self::new(
            404,
            "not-found",
            "Not found",
            "No route or invocation matches this request.",
        )
    }
    fn reply(self, request_id: &str) -> Reply {
        let mut value = json!({"type":format!("urn:agenticsre:problem:{}", self.code),"title":self.title,
            "status":self.status,"detail":self.detail,"request_id":request_id});
        if let Some(field) = self.field {
            value["errors"] = json!([{"field":field,"detail":self.detail}]);
        }
        response(
            self.status,
            "application/problem+json",
            Bytes::from(serde_json::to_vec(&value).expect("problem JSON")),
        )
    }
}

pub(crate) fn response(status: u16, content_type: &'static str, body: Bytes) -> Reply {
    let mut response = Response::builder().status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_SECURITY_POLICY, "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header(header::CONNECTION, "close")
        .body(Full::new(body)).expect("fixed valid response headers");
    if status == 429 {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "1".parse().expect("fixed header"));
    }
    response
}

fn json_response(status: u16, value: impl serde::Serialize) -> Reply {
    response(
        status,
        "application/json",
        Bytes::from(serde_json::to_vec(&value).expect("finite response")),
    )
}

fn unique_header(
    request: &Request<Incoming>,
    name: header::HeaderName,
) -> Result<Option<&str>, ApiError> {
    let mut values = request.headers().get_all(name).iter();
    let first = values.next().map(|value| value.to_str());
    if values.next().is_some() || first.as_ref().is_some_and(|v| v.is_err()) {
        return Err(ApiError::new(
            400,
            "invalid-header",
            "Invalid headers",
            "Control headers must be unique ASCII values.",
        ));
    }
    Ok(first.map(Result::unwrap))
}

fn boundary(request: &Request<Incoming>, app: &App) -> Result<(), ApiError> {
    if request.uri().authority().is_some()
        || request.uri().scheme().is_some()
        || unique_header(request, header::HOST)? != Some(app.authority.as_str())
    {
        return Err(ApiError::forbidden(
            "invalid-host",
            "Use the exact loopback authority printed by the launcher.",
        ));
    }
    let origin = unique_header(request, header::ORIGIN)?;
    let mutation = !matches!(*request.method(), hyper::Method::GET | hyper::Method::HEAD);
    if origin.is_some_and(|origin| origin != app.origin)
        || mutation && origin != Some(app.origin.as_str())
    {
        return Err(ApiError::forbidden(
            "invalid-origin",
            "This request requires the launcher's exact same origin.",
        ));
    }
    if request.uri().path().starts_with("/api/") {
        let bearer = unique_header(request, header::AUTHORIZATION)?
            .and_then(|value| value.strip_prefix("Bearer "));
        let valid = bearer.is_some_and(|bearer| {
            bearer.len() == app.token.len()
                && bearer
                    .bytes()
                    .zip(app.token.bytes())
                    .fold(0, |diff, (a, b)| diff | (a ^ b))
                    == 0
        });
        if !valid {
            return Err(ApiError::new(
                401,
                "invalid-token",
                "Launch link required",
                "Reopen the workbench launch link; session tokens remain only in browser memory.",
            ));
        }
        if mutation && unique_header(request, header::CONTENT_TYPE)? != Some("application/json") {
            return Err(ApiError::new(
                415,
                "unsupported-content-type",
                "JSON required",
                "Mutations require application/json.",
            ));
        }
    }
    Ok(())
}

async fn body(request: Request<Incoming>) -> Result<Value, ApiError> {
    const MAX: usize = 65536;
    if request.headers().get(header::CONTENT_ENCODING).is_some() {
        return Err(ApiError::new(
            415,
            "unsupported-encoding",
            "Unsupported body encoding",
            "Send uncompressed JSON.",
        ));
    }
    if request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|len| len > MAX as u64)
    {
        return Err(ApiError::new(
            413,
            "body-too-large",
            "Request is too large",
            "JSON requests are limited to64KiB.",
        ));
    }
    let collected = tokio::time::timeout(
        Duration::from_secs(2),
        Limited::new(request.into_body(), MAX).collect(),
    )
    .await
    .map_err(|_| {
        ApiError::new(
            408,
            "body-timeout",
            "Request body timed out",
            "Send the complete bounded body within two seconds.",
        )
    })?
    .map_err(|error| {
        if error.is::<http_body_util::LengthLimitError>() {
            ApiError::new(
                413,
                "body-too-large",
                "Request is too large",
                "JSON requests are limited to64KiB.",
            )
        } else {
            ApiError::new(
                400,
                "invalid-body",
                "Invalid request body",
                "The request body did not complete.",
            )
        }
    })?;
    workbench_core::request::parse_bounded_json(&collected.to_bytes(), 24).map_err(|_| {
        ApiError::new(
            400,
            "invalid-json",
            "Invalid JSON",
            "Use valid JSON with unique keys and at most24 container levels.",
        )
    })
}

fn pagination(query: Option<&str>) -> Result<(usize, usize), ApiError> {
    let mut limit = None;
    let mut offset = None;
    if let Some(query) = query {
        for part in query.split('&') {
            let (key, value) = part.split_once('=').ok_or_else(query_error)?;
            if value.is_empty() || value.len() > 2 || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(query_error());
            }
            let number = value.parse().map_err(|_| query_error())?;
            let target = match key {
                "limit" => &mut limit,
                "offset" => &mut offset,
                _ => return Err(query_error()),
            };
            if target.replace(number).is_some() {
                return Err(query_error());
            }
        }
    }
    let (limit, offset) = (limit.unwrap_or(50), offset.unwrap_or(0));
    if !(1..=50).contains(&limit) || offset > 49 {
        return Err(query_error());
    }
    Ok((limit, offset))
}
fn query_error() -> ApiError {
    ApiError::new(
        400,
        "invalid-query",
        "Invalid list query",
        "Supply unique limit=1..50 and offset=0..49 only.",
    )
}

async fn route(request: Request<Incoming>, app: &App) -> Result<Reply, ApiError> {
    boundary(&request, app)?;
    if request.uri().to_string().len() > 2048 {
        return Err(ApiError::not_found());
    }
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    if method == hyper::Method::GET {
        if request.headers().contains_key(header::TRANSFER_ENCODING)
            || request
                .headers()
                .get(header::CONTENT_LENGTH)
                .is_some_and(|v| v != "0")
        {
            return Err(ApiError::new(
                400,
                "unexpected-body",
                "Unexpected request body",
                "GET requests must not contain a body.",
            ));
        }
        if path == "/api/v1/runs" {
            let (limit, offset) = pagination(request.uri().query())?;
            return Ok(json_response(
                200,
                app.state.lock().expect("state lock").list(limit, offset),
            ));
        }
    }
    if path.starts_with("/api/") && request.uri().query().is_some() {
        return Err(query_error());
    }
    match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/session") => Ok(json_response(200, &app.session)),
        ("POST", "/api/v1/runs") => {
            let input = crate::input::normalize(body(request).await?, &app.grants)?;
            let (summary, work) = app.state.lock().expect("state lock").submit(input)?;
            if let Some(work) = work {
                let sent = app
                    .sender
                    .lock()
                    .expect("sender lock")
                    .as_ref()
                    .is_some_and(|sender| sender.try_send(work).is_ok());
                if !sent {
                    app.state.lock().expect("state lock").shutdown(15);
                    app.failed.store(true, std::sync::atomic::Ordering::Relaxed);
                    return Err(ApiError::new(
                        503,
                        "worker-unavailable",
                        "Worker unavailable",
                        "The worker failed before admission; this server session is closing.",
                    ));
                }
            }
            let mut response = json_response(202, &summary);
            response.headers_mut().insert(
                header::LOCATION,
                format!("/api/v1/runs/{}", summary.id)
                    .parse()
                    .expect("generated ID"),
            );
            Ok(response)
        }
        ("DELETE", "/api/v1/runs") => {
            require_empty(body(request).await?)?;
            Ok(json_response(
                200,
                app.state.lock().expect("state lock").clear(),
            ))
        }
        _ => {
            if let Some(tail) = path.strip_prefix("/api/v1/runs/") {
                let (id, suffix) = tail.split_once('/').unwrap_or((tail, ""));
                if id.is_empty()
                    || id.len() > 128
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                {
                    return Err(ApiError::not_found());
                }
                match (method.as_str(), suffix) {
                    ("GET", "") => {
                        return Ok(json_response(
                            200,
                            app.state.lock().expect("state lock").summary(id)?,
                        ));
                    }
                    ("GET", "receipt") => {
                        return Ok(response(
                            200,
                            "application/json",
                            app.state.lock().expect("state lock").receipt(id)?,
                        ));
                    }
                    ("POST", "cancel") => {
                        require_empty(body(request).await?)?;
                        let (status, summary) = app.state.lock().expect("state lock").cancel(id)?;
                        return Ok(json_response(status, summary));
                    }
                    _ => return Err(ApiError::not_found()),
                }
            }
            if method == hyper::Method::GET {
                let asset_path = if matches!(path.as_str(), "/checks" | "/history") {
                    "/"
                } else {
                    &path
                };
                if let Some((content_type, bytes)) = crate::assets::asset(asset_path) {
                    return Ok(response(200, content_type, Bytes::from_static(bytes)));
                }
            }
            Err(ApiError::not_found())
        }
    }
}

fn require_empty(value: Value) -> Result<(), ApiError> {
    if value.as_object().is_some_and(|map| map.is_empty()) {
        Ok(())
    } else {
        Err(ApiError::invalid(
            "body",
            "This operation requires an empty JSON object.",
        ))
    }
}

pub(crate) async fn handle(request: Request<Incoming>, app: Arc<App>) -> Result<Reply, Infallible> {
    let start = Instant::now();
    let id = workbench_core::result::new_id("http");
    let response = route(request, &app)
        .await
        .unwrap_or_else(|error| error.reply(&id));
    // Static metadata only: never log URLs, headers, request bodies or captured output.
    if let Some(sink) = app.diagnostics.lock().expect("diagnostics lock").as_mut() {
        use std::io::Write;
        let message = format!(
            "{}\n",
            json!({"event":"http_request","request_id":id,"status":response.status().as_u16(),"duration_ms":start.elapsed().as_millis()})
        );
        let _ = sink.write(message.as_bytes());
    }
    Ok(response)
}

pub(crate) fn overload() -> Reply {
    ApiError::new(
        429,
        "connection-limit",
        "Too many connections",
        "Retry after one second; connection limit reached.",
    )
    .reply(&workbench_core::result::new_id("http"))
}

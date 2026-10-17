//! Bounded projections of Grafana's JSON frame protocol, not an arbitrary JSON diff.
//!
//! Field names and labels identify metric series. Log IDs, frame names, display
//! configuration and transport metadata do not identify observations. Times are
//! integer nanoseconds throughout; JSON numbers are compared as exact decimal
//! values of the retained JSON representation, without an f64 conversion.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Number, Value, json};

const MAX_FRAMES: usize = 512;
const MAX_SERIES: usize = 512;
const MAX_FIELDS: usize = 128;
const MAX_OBSERVATIONS: usize = 10_000;
const MAX_LABELS: usize = 64;
const MAX_LABEL_BYTES: usize = 16 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
const NS_PER_MS: i128 = 1_000_000;

type Check<'a> = &'a mut dyn FnMut() -> Result<(), &'static str>;
type Labels = BTreeMap<String, String>;
type Reasons = BTreeSet<&'static str>;

#[derive(Clone, Copy)]
pub(super) enum Normalization {
    ExactWindow,
    WindowOffset,
}

#[derive(Clone, Copy)]
pub(super) struct Window {
    pub from_ns: i128,
    pub to_ns: i128,
}

pub(super) struct Comparison {
    pub summary: Value,
    pub details: Vec<Value>,
    pub partial: bool,
    pub reasons: Vec<&'static str>,
    pub omitted_details: u64,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn compare(
    kind: &str,
    before: &Value,
    after: &Value,
    before_window: Window,
    after_window: Window,
    normalization: Normalization,
    interval_ms: u64,
    max_details: usize,
    max_detail_bytes: usize,
    check: Check<'_>,
) -> Result<Comparison, &'static str> {
    check()?;
    let before_duration = before_window.to_ns.checked_sub(before_window.from_ns);
    let after_duration = after_window.to_ns.checked_sub(after_window.from_ns);
    if !before_duration.is_some_and(|duration| duration >= 0)
        || !after_duration.is_some_and(|duration| duration >= 0)
        || match normalization {
            Normalization::ExactWindow => {
                before_window.from_ns != after_window.from_ns
                    || before_window.to_ns != after_window.to_ns
            }
            Normalization::WindowOffset => before_duration != after_duration,
        }
    {
        return Err("incompatible_windows");
    }
    let mut details = Details::new(max_details, max_detail_bytes);
    match kind {
        "prometheus" => {
            if interval_ms == 0 {
                return Err("incompatible_sampling");
            }
            let before = metrics(before, before_window, normalization, check)?;
            let after = metrics(after, after_window, normalization, check)?;
            compare_metrics(before, after, interval_ms, &mut details, check)
        }
        "loki" => {
            let before = logs(before, before_window, normalization, check)?;
            let after = logs(after, after_window, normalization, check)?;
            compare_logs(before, after, &mut details, check)
        }
        _ => Err("unsupported_frame_kind"),
    }
}

/// Validate a historical frame without treating unrequested projected details as omissions.
pub(super) fn inspect(
    kind: &str,
    response: &Value,
    window: Window,
    check: Check<'_>,
) -> Result<Vec<&'static str>, &'static str> {
    check()?;
    let reasons = match kind {
        "prometheus" => metrics(response, window, Normalization::ExactWindow, check)?.reasons,
        "loki" => logs(response, window, Normalization::ExactWindow, check)?.reasons,
        _ => return Err("unsupported_frame_kind"),
    };
    check()?;
    Ok(reasons.into_iter().collect())
}

/// Deterministic projection of one historical response, without comparison or interpolation.
pub(super) fn replay(
    kind: &str,
    response: &Value,
    window: Window,
    max_details: usize,
    max_detail_bytes: usize,
    check: Check<'_>,
) -> Result<Comparison, &'static str> {
    check()?;
    let mut details = Details::new(max_details, max_detail_bytes);
    match kind {
        "prometheus" => {
            let observed = metrics(response, window, Normalization::ExactWindow, check)?;
            let mut summary = observed.counts();
            summary["finite"] = json!(observed.samples - observed.missing - observed.nonfinite);
            for (key, series) in &observed.series {
                for sample in series.samples.values() {
                    check()?;
                    let nonfinite = match &sample.value {
                        SampleValue::NonFinite(value) => Some(*value),
                        _ => None,
                    };
                    details.push(json!({"type":"metric_sample","series":key.json(),
                        "timestamp_ns":sample.original_ns.to_string(),"value":sample_json(Some(sample)),
                        "state":sample_state(Some(sample)),"nonfinite":nonfinite}))?;
                }
            }
            Ok(details.finish(summary, observed.reasons))
        }
        "loki" => {
            let observed = logs(response, window, Normalization::ExactWindow, check)?;
            let mut summary = observed.counts();
            summary["unique_entries"] = json!(observed.entries.len());
            summary["duplicate_entries"] = json!(observed.count - observed.entries.len());
            for (key, entry) in &observed.entries {
                check()?;
                details.push(json!({"type":"log_entry","labels":key.labels,"label_types":key.label_types,
                    "timestamp_ns":entry.original_ns.to_string(),"line":key.line,"count":entry.count}))?;
            }
            Ok(details.finish(summary, observed.reasons))
        }
        _ => Err("unsupported_frame_kind"),
    }
}

struct Details {
    values: Vec<Value>,
    max_count: usize,
    max_bytes: usize,
    bytes: usize,
    omitted: u64,
}

impl Details {
    fn new(max_count: usize, max_bytes: usize) -> Self {
        Self {
            values: Vec::new(),
            max_count,
            max_bytes,
            bytes: 2, // The JSON array delimiters are part of the byte bound.
            omitted: 0,
        }
    }

    fn push(&mut self, value: Value) -> Result<(), &'static str> {
        let bytes = serde_json::to_vec(&value)
            .map_err(|_| "invalid_comparison_detail")?
            .len()
            + usize::from(!self.values.is_empty());
        if self.omitted != 0
            || self.values.len() >= self.max_count
            || bytes > self.max_bytes.saturating_sub(self.bytes)
        {
            self.omitted += 1;
        } else {
            self.bytes += bytes;
            self.values.push(value);
        }
        Ok(())
    }

    fn finish(&mut self, summary: Value, mut reasons: Reasons) -> Comparison {
        if self.omitted != 0 {
            reasons.insert("details_omitted");
        }
        Comparison {
            summary,
            details: std::mem::take(&mut self.values),
            partial: !reasons.is_empty(),
            reasons: reasons.into_iter().collect(),
            omitted_details: self.omitted,
        }
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, &'static str> {
    if redacted(value) {
        return Err("redacted_observation");
    }
    let object = value.as_object().ok_or("unsupported_frame_shape")?;
    if object.keys().any(|key| key.contains("[REDACTED]")) {
        return Err("redacted_observation");
    }
    Ok(object)
}

fn redacted(value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.contains("[REDACTED]"))
        || value.get("redacted") == Some(&Value::Bool(true))
}

fn text(value: &Value, max: usize) -> Result<&str, &'static str> {
    if redacted(value) {
        return Err("redacted_observation");
    }
    let value = value.as_str().ok_or("unsupported_frame_shape")?;
    if value.len() > max {
        return Err("frame_limit_exceeded");
    }
    Ok(value)
}

fn labels(value: Option<&Value>) -> Result<Labels, &'static str> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    if redacted(value) {
        return Err("redacted_observation");
    }
    let values = object(value)?;
    if values.len() > MAX_LABELS {
        return Err("frame_limit_exceeded");
    }
    let mut result = BTreeMap::new();
    let mut size = 0;
    for (key, value) in values {
        if key.contains("[REDACTED]") {
            return Err("redacted_observation");
        }
        if key.is_empty() || key.len() > 256 {
            return Err("frame_limit_exceeded");
        }
        let value = text(value, 2048)?;
        size += key.len() + value.len();
        if size > MAX_LABEL_BYTES {
            return Err("frame_limit_exceeded");
        }
        result.insert(key.clone(), value.to_owned());
    }
    Ok(result)
}

fn merge_labels(into: &mut Labels, from: Labels) -> Result<(), &'static str> {
    for (key, value) in from {
        if into.get(&key).is_some_and(|previous| previous != &value) {
            return Err("ambiguous_log_labels");
        }
        into.insert(key, value);
    }
    if into.len() > MAX_LABELS
        || into.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>() > MAX_LABEL_BYTES
    {
        return Err("frame_limit_exceeded");
    }
    Ok(())
}

fn notices(value: &Value, reasons: &mut Reasons) -> Result<(), &'static str> {
    if let Some(notices) = value.get("notices") {
        let notices = notices.as_array().ok_or("unsupported_frame_metadata")?;
        if notices.len() > 64 {
            return Err("frame_limit_exceeded");
        }
        for notice in notices {
            match notice.get("severity").and_then(Value::as_str) {
                Some("warning" | "error") => {
                    reasons.insert("frame_notices");
                }
                Some("info") => {}
                _ => return Err("unsupported_frame_metadata"),
            }
        }
    }
    for key in ["truncated", "partial"] {
        if let Some(value) = value.get(key) {
            match value.as_bool() {
                Some(true) => {
                    reasons.insert("source_truncated");
                }
                Some(false) => {}
                None => return Err("unsupported_frame_metadata"),
            }
        }
    }
    Ok(())
}

fn response_frames<'a>(
    response: &'a Value,
    reasons: &mut Reasons,
) -> Result<&'a [Value], &'static str> {
    object(response)?;
    notices(response, reasons)?;
    let frames = response
        .get("frames")
        .and_then(Value::as_array)
        .ok_or("unsupported_frame_shape")?;
    if frames.len() > MAX_FRAMES {
        return Err("frame_limit_exceeded");
    }
    Ok(frames)
}

struct Frame<'a> {
    fields: &'a [Value],
    values: Vec<&'a [Value]>,
    nanos: Vec<Option<&'a [Value]>>,
    entities: Vec<BTreeMap<usize, &'static str>>,
    meta: Option<&'a Value>,
    rows: usize,
}

impl<'a> Frame<'a> {
    fn parse(
        value: &'a Value,
        kind: &str,
        reasons: &mut Reasons,
        check: Check<'_>,
    ) -> Result<Self, &'static str> {
        object(value)?;
        let schema = value.get("schema").ok_or("unsupported_frame_shape")?;
        object(schema)?;
        if let Some(ref_id) = schema.get("refId")
            && ref_id != "A"
        {
            return Err("unsupported_frame_shape");
        }
        let fields = schema
            .get("fields")
            .and_then(Value::as_array)
            .ok_or("unsupported_frame_shape")?;
        if fields.len() > MAX_FIELDS {
            return Err("frame_limit_exceeded");
        }
        let meta = schema.get("meta");
        if let Some(meta) = meta {
            object(meta)?;
            notices(meta, reasons)?;
            if let Some(custom) = meta.get("custom") {
                object(custom)?;
                notices(custom, reasons)?;
                if kind == "prometheus"
                    && custom
                        .get("resultType")
                        .is_some_and(|value| value != "matrix")
                {
                    return Err("unsupported_frame_profile");
                }
                if kind == "loki"
                    && custom
                        .get("frameType")
                        .is_some_and(|value| value != "LabeledTimeValues")
                {
                    return Err("unsupported_frame_profile");
                }
            }
            if let Some(frame_type) = meta.get("type") {
                let supported = match kind {
                    "prometheus" => matches!(
                        frame_type.as_str(),
                        Some("timeseries-wide" | "timeseries-multi")
                    ),
                    "loki" => frame_type == "log-lines",
                    _ => false,
                };
                if !supported {
                    return Err("unsupported_frame_profile");
                }
            }
            if let Some(version) = meta.get("typeVersion")
                && version
                    != &if kind == "loki" || fields.is_empty() {
                        json!([0, 0])
                    } else {
                        json!([0, 1])
                    }
            {
                return Err("unsupported_frame_profile");
            }
        }
        let data = value.get("data").ok_or("unsupported_frame_shape")?;
        if object(data)?
            .keys()
            .any(|key| !matches!(key.as_str(), "values" | "nanos" | "entities"))
        {
            return Err("unsupported_frame_shape");
        }
        let columns = data
            .get("values")
            .and_then(Value::as_array)
            .ok_or("unsupported_frame_shape")?;
        if columns.len() != fields.len() {
            return Err("unsupported_frame_shape");
        }
        let mut values = Vec::with_capacity(columns.len());
        for (field, column) in fields.iter().zip(columns) {
            check()?;
            object(field)?;
            text(field.get("name").ok_or("unsupported_frame_shape")?, 256)?;
            text(field.get("type").ok_or("unsupported_frame_shape")?, 32)?;
            if let Some(type_info) = field.get("typeInfo") {
                object(type_info)?;
                let frame_type = type_info
                    .get("frame")
                    .and_then(Value::as_str)
                    .ok_or("unsupported_frame_profile")?;
                let supported = match field["type"].as_str() {
                    Some("time") => frame_type == "time.Time",
                    Some("number") => matches!(
                        frame_type,
                        "float32"
                            | "float64"
                            | "int8"
                            | "int16"
                            | "int32"
                            | "int64"
                            | "uint8"
                            | "uint16"
                            | "uint32"
                            | "uint64"
                    ),
                    Some("string") => frame_type == "string",
                    Some("other") => frame_type == "json.RawMessage",
                    _ => false,
                };
                if !supported
                    || type_info
                        .get("nullable")
                        .is_some_and(|value| !value.is_boolean())
                {
                    return Err("unsupported_frame_profile");
                }
            }
            let column = column.as_array().ok_or("unsupported_frame_shape")?;
            if column.len() > MAX_OBSERVATIONS {
                return Err("frame_limit_exceeded");
            }
            if values
                .first()
                .is_some_and(|first: &&[Value]| first.len() != column.len())
            {
                return Err("unsupported_frame_shape");
            }
            values.push(column.as_slice());
        }
        let rows = values.first().map(|column| column.len()).unwrap_or(0);
        let mut nanos = vec![None; fields.len()];
        if let Some(offsets) = data.get("nanos") {
            let offsets = offsets.as_array().ok_or("unsupported_frame_shape")?;
            if offsets.len() != fields.len() {
                return Err("unsupported_frame_shape");
            }
            for (index, offsets) in offsets.iter().enumerate() {
                check()?;
                if offsets.is_null() {
                    continue;
                }
                if fields[index]["type"] != "time" {
                    return Err("unsupported_frame_shape");
                }
                let offsets = offsets.as_array().ok_or("unsupported_frame_shape")?;
                if offsets.len() != rows {
                    return Err("unsupported_frame_shape");
                }
                for offset in offsets {
                    check()?;
                    if redacted(offset) {
                        return Err("redacted_observation");
                    }
                    if !offset
                        .as_i64()
                        .is_some_and(|offset| (-999_999..=999_999).contains(&offset))
                    {
                        return Err("unsupported_timestamp");
                    }
                }
                nanos[index] = Some(offsets.as_slice());
            }
        }
        let mut entities = vec![BTreeMap::new(); fields.len()];
        if let Some(encoded) = data.get("entities") {
            let encoded = encoded.as_array().ok_or("unsupported_frame_shape")?;
            if encoded.len() != fields.len() {
                return Err("unsupported_frame_shape");
            }
            for (column, encoded) in encoded.iter().enumerate() {
                check()?;
                if encoded.is_null() {
                    continue;
                }
                if fields[column]["type"] != "number" {
                    return Err("unsupported_frame_shape");
                }
                for (name, indexes) in object(encoded)? {
                    let name = match name.as_str() {
                        "NaN" => "NaN",
                        "Inf" => "Inf",
                        "NegInf" => "NegInf",
                        _ => return Err("unsupported_nonfinite_encoding"),
                    };
                    let indexes = indexes.as_array().ok_or("unsupported_frame_shape")?;
                    if indexes.len() > rows {
                        return Err("unsupported_nonfinite_encoding");
                    }
                    for index in indexes {
                        check()?;
                        let index = index
                            .as_u64()
                            .and_then(|value| usize::try_from(value).ok())
                            .ok_or("unsupported_nonfinite_encoding")?;
                        if index >= rows
                            || !values[column][index].is_null()
                            || entities[column].insert(index, name).is_some()
                        {
                            return Err("unsupported_nonfinite_encoding");
                        }
                    }
                }
            }
        }
        Ok(Self {
            fields,
            values,
            nanos,
            entities,
            meta,
            rows,
        })
    }

    fn timestamp(&self, column: usize, row: usize) -> Result<i128, &'static str> {
        let value = &self.values[column][row];
        if redacted(value) {
            return Err("redacted_observation");
        }
        let milliseconds = value.as_i64().ok_or("unsupported_timestamp")?;
        let offset = self.nanos[column]
            .map(|offsets| offsets[row].as_i64().expect("validated nanosecond offset"))
            .unwrap_or(0);
        Ok(i128::from(milliseconds) * NS_PER_MS + i128::from(offset))
    }
}

fn normalized_time(
    time: i128,
    window: Window,
    normalization: Normalization,
) -> Result<i128, &'static str> {
    if time < window.from_ns || time > window.to_ns {
        return Err("observation_outside_window");
    }
    match normalization {
        Normalization::ExactWindow => Ok(time),
        Normalization::WindowOffset => time
            .checked_sub(window.from_ns)
            .ok_or("unsupported_timestamp"),
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SeriesKey {
    field: String,
    labels: Labels,
}

impl SeriesKey {
    fn json(&self) -> Value {
        json!({"field":self.field,"labels":self.labels})
    }
}

enum SampleValue {
    Finite(Number),
    Missing,
    NonFinite(&'static str),
}

struct Sample {
    original_ns: i128,
    value: SampleValue,
}

struct Series {
    samples: BTreeMap<i128, Sample>,
    interval_ms: Option<u64>,
}

#[derive(Default)]
struct Metrics {
    series: BTreeMap<SeriesKey, Series>,
    samples: usize,
    missing: usize,
    nonfinite: usize,
    sampling_gaps: u64,
    reasons: Reasons,
}

impl Metrics {
    fn counts(&self) -> Value {
        json!({"series":self.series.len(),"samples":self.samples,"missing":self.missing,
            "nonfinite":self.nonfinite,"sampling_gaps":self.sampling_gaps})
    }

    fn intervals(&self) -> BTreeSet<Option<u64>> {
        self.series
            .values()
            .map(|series| series.interval_ms)
            .collect()
    }
}

fn field_config(field: &Value) -> Result<Option<&Map<String, Value>>, &'static str> {
    field.get("config").map(object).transpose()
}

fn metric_interval(field: &Value) -> Result<Option<u64>, &'static str> {
    field_config(field)?
        .and_then(|config| config.get("interval"))
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or("incompatible_sampling")
        })
        .transpose()
}

fn metric_unit(field: &Value) -> Result<(), &'static str> {
    if let Some(unit) = field_config(field)?.and_then(|config| config.get("unit"))
        && !matches!(unit.as_str(), Some("" | "none" | "dimensionless"))
    {
        return Err("incompatible_units");
    }
    Ok(())
}

fn metrics(
    response: &Value,
    window: Window,
    normalization: Normalization,
    check: Check<'_>,
) -> Result<Metrics, &'static str> {
    let mut result = Metrics::default();
    for value in response_frames(response, &mut result.reasons)? {
        check()?;
        let frame = Frame::parse(value, "prometheus", &mut result.reasons, check)?;
        if frame.fields.is_empty() {
            result.reasons.insert("empty_observations");
            continue;
        }
        let times: Vec<_> = frame
            .fields
            .iter()
            .enumerate()
            .filter(|(_, field)| field["type"] == "time")
            .map(|(i, _)| i)
            .collect();
        if times.len() != 1 || frame.fields.len() < 2 {
            return Err("unsupported_frame_profile");
        }
        let time_column = times[0];
        let interval_ms = metric_interval(&frame.fields[time_column])?;
        if let Some(calculated) = frame
            .meta
            .and_then(|meta| meta.pointer("/custom/calculatedMinStep"))
        {
            let calculated = calculated
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or("incompatible_sampling")?;
            if Some(calculated) != interval_ms {
                return Err("incompatible_sampling");
            }
        }
        if !labels(frame.fields[time_column].get("labels"))?.is_empty() {
            return Err("ambiguous_series_identity");
        }
        let mut timestamps = Vec::with_capacity(frame.rows);
        for row in 0..frame.rows {
            check()?;
            let time = frame.timestamp(time_column, row)?;
            let key = normalized_time(time, window, normalization)?;
            if timestamps
                .last()
                .is_some_and(|(previous, _)| *previous >= key)
            {
                return Err("ambiguous_sample_identity");
            }
            timestamps.push((key, time));
        }
        for (column, field) in frame.fields.iter().enumerate() {
            check()?;
            if column == time_column {
                continue;
            }
            if field["type"] != "number" {
                return Err("unsupported_frame_profile");
            }
            metric_unit(field)?;
            if let Some(field_interval) = metric_interval(field)?
                && Some(field_interval) != interval_ms
            {
                return Err("incompatible_sampling");
            }
            let key = SeriesKey {
                field: text(&field["name"], 256)?.to_owned(),
                labels: labels(field.get("labels"))?,
            };
            if key.field.is_empty() {
                return Err("ambiguous_series_identity");
            }
            if !result.series.contains_key(&key) && result.series.len() >= MAX_SERIES {
                return Err("frame_limit_exceeded");
            }
            if result.samples + frame.rows > MAX_OBSERVATIONS {
                return Err("frame_limit_exceeded");
            }
            let series = result.series.entry(key).or_insert_with(|| Series {
                samples: BTreeMap::new(),
                interval_ms,
            });
            if series.interval_ms != interval_ms {
                return Err("incompatible_sampling");
            }
            for (row, (key, original_ns)) in timestamps.iter().enumerate() {
                check()?;
                let value = &frame.values[column][row];
                if redacted(value) {
                    return Err("redacted_observation");
                }
                let value = if let Some(entity) = frame.entities[column].get(&row) {
                    result.nonfinite += 1;
                    result.reasons.insert("nonfinite_samples");
                    SampleValue::NonFinite(entity)
                } else if value.is_null() {
                    result.missing += 1;
                    result.reasons.insert("missing_samples");
                    SampleValue::Missing
                } else if let Some(number) = value.as_number() {
                    SampleValue::Finite(number.clone())
                } else {
                    return Err("unsupported_sample_value");
                };
                if series
                    .samples
                    .insert(
                        *key,
                        Sample {
                            original_ns: *original_ns,
                            value,
                        },
                    )
                    .is_some()
                {
                    return Err("ambiguous_sample_identity");
                }
                result.samples += 1;
            }
        }
    }
    if result.samples == 0 {
        result.reasons.insert("empty_observations");
    }
    for series in result.series.values() {
        check()?;
        if let Some(interval) = series.interval_ms {
            let step = i128::from(interval) * NS_PER_MS;
            let mut previous = None;
            for time in series.samples.keys() {
                check()?;
                if let Some(previous) = previous {
                    let delta = time - previous;
                    if delta % step != 0 {
                        return Err("incompatible_sampling");
                    }
                    let gaps =
                        u64::try_from(delta / step - 1).map_err(|_| "frame_limit_exceeded")?;
                    result.sampling_gaps = result
                        .sampling_gaps
                        .checked_add(gaps)
                        .ok_or("frame_limit_exceeded")?;
                }
                previous = Some(*time);
            }
        }
    }
    if result.sampling_gaps != 0 {
        result.reasons.insert("sampling_gaps");
    }
    Ok(result)
}

// Number's shortest decimal spelling retains integer distinctions that an f64
// cast would erase. Normalize only insignificant decimal zeros and exponents.
fn decimal(number: &Number) -> String {
    let representation = number.to_string();
    let (mantissa, exponent) = representation
        .split_once(['e', 'E'])
        .map(|(mantissa, exponent)| {
            (
                mantissa,
                exponent.parse::<i32>().expect("JSON number exponent"),
            )
        })
        .unwrap_or((&representation, 0));
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let fractional = mantissa
        .split_once('.')
        .map(|(_, fraction)| fraction.len())
        .unwrap_or(0);
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".into();
    }
    let significant = digits.trim_end_matches('0');
    let exponent = exponent - fractional as i32 + (digits.len() - significant.len()) as i32;
    format!(
        "{}{significant}e{exponent}",
        if negative { "-" } else { "" }
    )
}

fn sample_json(sample: Option<&Sample>) -> Value {
    match sample.map(|sample| &sample.value) {
        Some(SampleValue::Finite(number)) => Value::Number(number.clone()),
        _ => Value::Null,
    }
}

fn sample_state(sample: Option<&Sample>) -> &'static str {
    match sample.map(|sample| &sample.value) {
        Some(SampleValue::Finite(_)) => "finite",
        Some(SampleValue::Missing) => "null",
        Some(SampleValue::NonFinite(_)) => "non_finite",
        None => "absent",
    }
}

#[derive(Default)]
struct MetricChanges {
    series_added: usize,
    series_removed: usize,
    samples_added: usize,
    samples_removed: usize,
    samples_changed: usize,
    samples_missing: usize,
    samples_unchanged: usize,
}

fn compare_metrics(
    before: Metrics,
    after: Metrics,
    interval_ms: u64,
    details: &mut Details,
    check: Check<'_>,
) -> Result<Comparison, &'static str> {
    // An empty side has no declared series interval to contradict the other.
    if !before.series.is_empty()
        && !after.series.is_empty()
        && before.intervals() != after.intervals()
    {
        return Err("incompatible_sampling");
    }
    for (key, series) in &before.series {
        check()?;
        if let Some(other) = after.series.get(key)
            && series.interval_ms != other.interval_ms
        {
            return Err("incompatible_sampling");
        }
        if let Some(other) = after.series.get(key)
            && !series.samples.is_empty()
            && !other.samples.is_empty()
            && !series
                .samples
                .keys()
                .any(|time| other.samples.contains_key(time))
        {
            return Err("incompatible_sampling");
        }
    }
    let mut reasons: Reasons = before.reasons.union(&after.reasons).copied().collect();
    let mut changes = MetricChanges::default();
    let keys: BTreeSet<_> = before.series.keys().chain(after.series.keys()).collect();
    for key in keys {
        check()?;
        let left = before.series.get(key);
        let right = after.series.get(key);
        if left.is_none() {
            changes.series_added += 1;
            details.push(json!({"type":"series_added","series":key.json()}))?;
        } else if right.is_none() {
            changes.series_removed += 1;
            details.push(json!({"type":"series_removed","series":key.json()}))?;
        }
        let timestamps: BTreeSet<_> = left
            .into_iter()
            .flat_map(|series| series.samples.keys())
            .chain(right.into_iter().flat_map(|series| series.samples.keys()))
            .collect();
        for timestamp in timestamps {
            check()?;
            let l = left.and_then(|series| series.samples.get(timestamp));
            let r = right.and_then(|series| series.samples.get(timestamp));
            let unknown = matches!(
                l.map(|sample| &sample.value),
                Some(SampleValue::Missing | SampleValue::NonFinite(_))
            ) || matches!(
                r.map(|sample| &sample.value),
                Some(SampleValue::Missing | SampleValue::NonFinite(_))
            );
            let gap = left.is_some() && right.is_some() && (l.is_none() || r.is_none());
            if unknown || gap {
                changes.samples_missing += 1;
                reasons.insert("missing_samples");
            }
            let event = if unknown {
                "sample_missing"
            } else {
                match (l.map(|sample| &sample.value), r.map(|sample| &sample.value)) {
                    (Some(SampleValue::Finite(a)), Some(SampleValue::Finite(b)))
                        if decimal(a) == decimal(b) =>
                    {
                        changes.samples_unchanged += 1;
                        continue;
                    }
                    (Some(_), Some(_)) => {
                        changes.samples_changed += 1;
                        "sample_changed"
                    }
                    (None, Some(_)) => {
                        changes.samples_added += 1;
                        "sample_added"
                    }
                    (Some(_), None) => {
                        changes.samples_removed += 1;
                        "sample_removed"
                    }
                    (None, None) => unreachable!("timestamp comes from one source"),
                }
            };
            let mut value = json!({"type":event,"series":key.json(),"timestamp_ns":timestamp.to_string(),
                "before":sample_json(l),"after":sample_json(r),"before_state":sample_state(l),"after_state":sample_state(r),
                "before_timestamp_ns":l.map(|sample| sample.original_ns.to_string()),
                "after_timestamp_ns":r.map(|sample| sample.original_ns.to_string())});
            for (name, sample) in [("before_nonfinite", l), ("after_nonfinite", r)] {
                if let Some(Sample {
                    value: SampleValue::NonFinite(entity),
                    ..
                }) = sample
                {
                    value[name] = json!(entity);
                }
            }
            details.push(value)?;
        }
    }
    check()?;
    Ok(details.finish(json!({"before":before.counts(),"after":after.counts(),
        "series_added":changes.series_added,"series_removed":changes.series_removed,
        "samples_added":changes.samples_added,"samples_removed":changes.samples_removed,
        "samples_changed":changes.samples_changed,"samples_missing":changes.samples_missing,
        "samples_unchanged":changes.samples_unchanged,
        "sampling":{"request_interval_ms":interval_ms,"before_declared_intervals_ms":before.intervals(),
            "after_declared_intervals_ms":after.intervals()}}), reasons))
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct LogKey {
    labels: Labels,
    label_types: Labels,
    timestamp_ns: i128,
    line: String,
}

struct LogCount {
    count: usize,
    original_ns: i128,
}

#[derive(Default)]
struct Logs {
    entries: BTreeMap<LogKey, LogCount>,
    streams: BTreeSet<(Labels, Labels)>,
    count: usize,
    reasons: Reasons,
}

impl Logs {
    fn counts(&self) -> Value {
        json!({"entries":self.count,"label_sets":self.streams.len()})
    }
}

fn logs(
    response: &Value,
    window: Window,
    normalization: Normalization,
    check: Check<'_>,
) -> Result<Logs, &'static str> {
    let mut result = Logs::default();
    for value in response_frames(response, &mut result.reasons)? {
        check()?;
        let frame = Frame::parse(value, "loki", &mut result.reasons, check)?;
        let mut columns = BTreeMap::new();
        let mut static_labels = Labels::new();
        for (index, field) in frame.fields.iter().enumerate() {
            check()?;
            let name = text(&field["name"], 256)?;
            if columns.insert(name, index).is_some() {
                return Err("unsupported_frame_profile");
            }
            let expected = match name {
                "Time" | "timestamp" => "time",
                "Line" | "body" | "tsNs" | "id" => "string",
                "labels" | "labelTypes" => "other",
                _ => return Err("unsupported_frame_profile"),
            };
            if field["type"] != expected {
                return Err("unsupported_frame_profile");
            }
            merge_labels(&mut static_labels, labels(field.get("labels"))?)?;
        }
        let (time_column, line_column) = match (
            columns.get("Time"),
            columns.get("Line"),
            columns.get("timestamp"),
            columns.get("body"),
        ) {
            (Some(time), Some(line), None, None) => (*time, *line),
            (None, None, Some(time), Some(line)) => (*time, *line),
            _ => return Err("unsupported_frame_profile"),
        };
        if result.count + frame.rows > MAX_OBSERVATIONS {
            return Err("frame_limit_exceeded");
        }
        if let Some(limit) = frame.meta.and_then(|meta| meta.pointer("/custom/limit")) {
            let limit = limit
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or("unsupported_frame_metadata")?;
            if frame.rows as u64 >= limit {
                result.reasons.insert("source_truncated");
            }
        }
        for row in 0..frame.rows {
            check()?;
            let base_time = frame.timestamp(time_column, row)?;
            let original_ns = if let Some(column) = columns.get("tsNs") {
                let precise = text(&frame.values[*column][row], 40)?;
                let precise = precise
                    .parse::<i128>()
                    .map_err(|_| "unsupported_timestamp")?;
                let same_millisecond =
                    precise.div_euclid(NS_PER_MS) == base_time.div_euclid(NS_PER_MS);
                if !same_millisecond || (frame.nanos[time_column].is_some() && precise != base_time)
                {
                    return Err("inconsistent_timestamps");
                }
                precise
            } else {
                base_time
            };
            let timestamp_ns = normalized_time(original_ns, window, normalization)?;
            let line = text(&frame.values[line_column][row], MAX_LINE_BYTES)?.to_owned();
            let mut entry_labels = static_labels.clone();
            if let Some(column) = columns.get("labels") {
                merge_labels(
                    &mut entry_labels,
                    labels(Some(&frame.values[*column][row]))?,
                )?;
            }
            let label_types = columns
                .get("labelTypes")
                .map(|column| labels(Some(&frame.values[*column][row])))
                .transpose()?
                .unwrap_or_default();
            if label_types.iter().any(|(label, kind)| {
                !entry_labels.contains_key(label) || !matches!(kind.as_str(), "I" | "P" | "S")
            }) {
                return Err("unsupported_label_types");
            }
            if let Some(column) = columns.get("id") {
                // IDs are generated presentation aids. Validate the column type,
                // but their values (including redaction) never identify a log.
                if !frame.values[*column][row].is_string() {
                    return Err("unsupported_frame_shape");
                }
            }
            result
                .streams
                .insert((entry_labels.clone(), label_types.clone()));
            if result.streams.len() > MAX_SERIES {
                return Err("frame_limit_exceeded");
            }
            let key = LogKey {
                labels: entry_labels,
                label_types,
                timestamp_ns,
                line,
            };
            result
                .entries
                .entry(key)
                .and_modify(|entry| entry.count += 1)
                .or_insert(LogCount {
                    count: 1,
                    original_ns,
                });
            result.count += 1;
        }
    }
    Ok(result)
}

fn compare_logs(
    before: Logs,
    after: Logs,
    details: &mut Details,
    check: Check<'_>,
) -> Result<Comparison, &'static str> {
    let reasons = before.reasons.union(&after.reasons).copied().collect();
    let keys: BTreeSet<_> = before.entries.keys().chain(after.entries.keys()).collect();
    let mut added = 0;
    let mut removed = 0;
    let mut unchanged = 0;
    for key in keys {
        check()?;
        let left = before.entries.get(key);
        let right = after.entries.get(key);
        let l = left.map(|entry| entry.count).unwrap_or(0);
        let r = right.map(|entry| entry.count).unwrap_or(0);
        unchanged += l.min(r);
        let (event, count) = if l < r {
            added += r - l;
            ("entry_added", r - l)
        } else if l > r {
            removed += l - r;
            ("entry_removed", l - r)
        } else {
            continue;
        };
        details.push(
            json!({"type":event,"labels":key.labels,"label_types":key.label_types,
            "timestamp_ns":key.timestamp_ns.to_string(),"line":key.line,"count":count,
            "before_count":l,"after_count":r,
            "before_timestamp_ns":left.map(|entry| entry.original_ns.to_string()),
            "after_timestamp_ns":right.map(|entry| entry.original_ns.to_string())}),
        )?;
    }
    check()?;
    Ok(details.finish(
        json!({"before":before.counts(),"after":after.counts(),
        "entries_added":added,"entries_removed":removed,"entries_unchanged":unchanged}),
        reasons,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Window = Window {
        from_ns: 0,
        to_ns: 10_000_000_000,
    };

    fn metric(times: Value, values: Value, instance: &str) -> Value {
        json!({"schema":{"refId":"A","meta":{"type":"timeseries-multi","typeVersion":[0,1],"custom":{"resultType":"matrix"}},
            "fields":[{"name":"Time","type":"time","typeInfo":{"frame":"time.Time"}},
                {"name":"up","type":"number","labels":{"job":"web","instance":instance},"typeInfo":{"frame":"float64"}}]},
            "data":{"values":[times,values]}})
    }

    fn response(frames: Vec<Value>) -> Value {
        json!({"status":200,"frames":frames})
    }

    fn run(kind: &str, before: &Value, after: &Value) -> Result<Comparison, &'static str> {
        compare(
            kind,
            before,
            after,
            WINDOW,
            WINDOW,
            Normalization::ExactWindow,
            1000,
            1000,
            1024 * 1024,
            &mut || Ok(()),
        )
    }

    fn legacy_logs(times: Vec<i64>, nanos: Vec<i64>, lines: Vec<&str>) -> Value {
        let labels: Vec<_> = times
            .iter()
            .map(|_| json!({"job":"web","host":"one"}))
            .collect();
        let precise: Vec<_> = times
            .iter()
            .zip(&nanos)
            .map(|(time, nano)| (i128::from(*time) * NS_PER_MS + i128::from(*nano)).to_string())
            .collect();
        let ids: Vec<_> = times
            .iter()
            .enumerate()
            .map(|(index, _)| format!("id-{index}"))
            .collect();
        json!({"schema":{"meta":{"typeVersion":[0,0],"custom":{"frameType":"LabeledTimeValues"}},"fields":[
            {"name":"labels","type":"other","typeInfo":{"frame":"json.RawMessage"}},
            {"name":"Time","type":"time"},{"name":"Line","type":"string"},
            {"name":"tsNs","type":"string"},{"name":"id","type":"string"}]},
            "data":{"values":[labels,times,lines,precise,ids],"nanos":[null,nanos,null,null,null]}})
    }

    fn dataplane(mut legacy: Value) -> Value {
        legacy["schema"]["meta"] = json!({"type":"log-lines","typeVersion":[0,0]});
        legacy["schema"]["fields"][1]["name"] = json!("timestamp");
        legacy["schema"]["fields"][2]["name"] = json!("body");
        legacy["schema"]["fields"].as_array_mut().unwrap().remove(3);
        legacy["data"]["values"].as_array_mut().unwrap().remove(3);
        legacy["data"]["nanos"].as_array_mut().unwrap().remove(3);
        legacy
    }

    #[test]
    fn inspection_validates_frames_without_artificial_detail_omissions() {
        let mut frame = metric(json!([1000, 2000]), json!([1, 0]), "a");
        assert!(
            inspect(
                "prometheus",
                &response(vec![frame.clone()]),
                WINDOW,
                &mut || Ok(())
            )
            .unwrap()
            .is_empty()
        );
        frame["data"]["values"][1][1] = Value::Null;
        assert_eq!(
            inspect(
                "prometheus",
                &response(vec![frame.clone()]),
                WINDOW,
                &mut || Ok(())
            )
            .unwrap(),
            vec!["missing_samples"]
        );
        frame["data"]["values"][1][0] = json!("[REDACTED]");
        assert_eq!(
            inspect("prometheus", &response(vec![frame]), WINDOW, &mut || Ok(())).err(),
            Some("redacted_observation")
        );
        let mut logs = legacy_logs(vec![1000], vec![123], vec!["line"]);
        logs["schema"]["meta"]["type"] = json!("unsupported-frame");
        assert_eq!(
            inspect("loki", &response(vec![logs]), WINDOW, &mut || Ok(())).err(),
            Some("unsupported_frame_profile")
        );
    }

    #[test]
    fn replay_counts_finite_missing_and_nonfinite_with_original_nanoseconds() {
        let mut frame = metric(
            json!([1000, 2000, 3000]),
            json!([9007199254740993_u64, null, null]),
            "a",
        );
        frame["data"]["nanos"] = json!([[123, 0, 0], null]);
        frame["data"]["entities"] = json!([null,{"NaN":[2]}]);
        let result = replay(
            "prometheus",
            &response(vec![frame]),
            WINDOW,
            100,
            10000,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(result.summary["samples"], 3);
        assert_eq!(result.summary["finite"], 1);
        assert_eq!(result.summary["missing"], 1);
        assert_eq!(result.summary["nonfinite"], 1);
        assert_eq!(result.details[0]["timestamp_ns"], "1000000123");
        assert_eq!(result.details[0]["value"], json!(9007199254740993_u64));
        assert_eq!(result.details[1]["state"], "null");
        assert_eq!(result.details[2]["nonfinite"], "NaN");
        assert!(result.partial);
    }

    #[test]
    fn replay_duplicates_are_multiplicity_not_new_events_and_order_is_canonical() {
        let a = legacy_logs(vec![1000, 1000], vec![123, 123], vec!["same", "same"]);
        let b = legacy_logs(vec![2000], vec![456], vec!["later"]);
        let result = replay(
            "loki",
            &response(vec![a.clone(), b.clone()]),
            WINDOW,
            100,
            10000,
            &mut || Ok(()),
        )
        .unwrap();
        let reordered = replay(
            "loki",
            &response(vec![b, a]),
            WINDOW,
            100,
            10000,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(result.summary, reordered.summary);
        assert_eq!(result.details, reordered.details);
        assert_eq!(result.summary["entries"], 3);
        assert_eq!(result.summary["unique_entries"], 2);
        assert_eq!(result.summary["duplicate_entries"], 1);
        assert_eq!(result.details[0]["count"], 2);
        assert_eq!(result.details[0]["timestamp_ns"], "1000000123");
        assert!(!result.partial);
    }

    #[test]
    fn replay_detail_bounds_preserve_full_counts_and_cancellation_is_checked() {
        let data = response(vec![metric(
            json!([1000, 2000, 3000]),
            json!([1, 0, 1]),
            "a",
        )]);
        let result = replay("prometheus", &data, WINDOW, 1, 10000, &mut || Ok(())).unwrap();
        assert_eq!(result.summary["finite"], 3);
        assert_eq!(result.details.len(), 1);
        assert_eq!(result.omitted_details, 2);
        let result = replay("prometheus", &data, WINDOW, 100, 2, &mut || Ok(())).unwrap();
        assert_eq!(result.omitted_details, 3);
        assert!(result.details.is_empty());
        let mut calls = 0;
        assert_eq!(
            replay("prometheus", &data, WINDOW, 100, 10000, &mut || {
                calls += 1;
                if calls > 8 { Err("cancelled") } else { Ok(()) }
            })
            .err(),
            Some("cancelled")
        );
    }

    #[test]
    fn wide_and_multi_profiles_compare_the_same_canonical_series() {
        let a = metric(json!([1000, 2000]), json!([1, 0]), "a");
        let b = metric(json!([1000, 2000]), json!([0, 1]), "b");
        let mut wide = a.clone();
        wide["schema"]["meta"]["type"] = json!("timeseries-wide");
        wide["schema"]["fields"]
            .as_array_mut()
            .unwrap()
            .push(b["schema"]["fields"][1].clone());
        wide["data"]["values"]
            .as_array_mut()
            .unwrap()
            .push(b["data"]["values"][1].clone());
        let result = run("prometheus", &response(vec![wide]), &response(vec![b, a])).unwrap();
        assert!(!result.partial);
        assert_eq!(result.summary["samples_unchanged"], 4);
        assert_eq!(result.summary["before"]["series"], 2);
        assert!(result.details.is_empty());
    }

    #[test]
    fn hand_calculated_metric_changes_preserve_additions_removals_and_nulls() {
        let before = response(vec![
            metric(json!([1000, 2000, 3000]), json!([1, 0, null]), "a"),
            metric(json!([1000]), json!([1]), "removed"),
        ]);
        let after = response(vec![
            metric(json!([1000, 2000, 4000]), json!([0, 0, 1]), "a"),
            metric(json!([1000]), json!([1]), "added"),
        ]);
        let result = run("prometheus", &before, &after).unwrap();
        assert!(result.partial);
        for (field, expected) in [
            ("series_added", 1),
            ("series_removed", 1),
            ("samples_changed", 1),
            ("samples_added", 2),
            ("samples_removed", 1),
            ("samples_missing", 2),
            ("samples_unchanged", 1),
        ] {
            assert_eq!(result.summary[field], expected, "{field}");
        }
        let missing = result
            .details
            .iter()
            .find(|event| event["type"] == "sample_missing")
            .unwrap();
        assert_eq!(missing["timestamp_ns"], "3000000000");
        assert_eq!(missing["before_state"], "null");
        assert_eq!(missing["after_state"], "absent");
    }

    #[test]
    fn numbers_beyond_binary_float_integer_precision_stay_distinct() {
        let before = response(vec![metric(
            json!([1000, 2000, 3000, 4000]),
            json!([9007199254740992_u64, 1, 1e-6, -0.0]),
            "a",
        )]);
        let after = response(vec![metric(
            json!([1000, 2000, 3000, 4000]),
            json!([9007199254740993_u64, 1.0, 0.000001, 0]),
            "a",
        )]);
        let result = run("prometheus", &before, &after).unwrap();
        assert_eq!(result.summary["samples_changed"], 1);
        assert_eq!(result.summary["samples_unchanged"], 3);
        assert_eq!(result.details[0]["before"], json!(9007199254740992_u64));
        assert_eq!(result.details[0]["after"], json!(9007199254740993_u64));
    }

    #[test]
    fn nonfinite_entities_are_distinct_from_missing_and_never_equal_finite_samples() {
        let mut frame = metric(
            json!([1000, 2000, 3000, 4000]),
            json!([null, null, null, null]),
            "a",
        );
        frame["data"]["entities"] = json!([null,{"NaN":[0],"Inf":[1],"NegInf":[2]}]);
        let data = response(vec![frame.clone()]);
        let result = run("prometheus", &data, &data).unwrap();
        assert!(result.partial);
        assert_eq!(result.summary["before"]["missing"], 1);
        assert_eq!(result.summary["before"]["nonfinite"], 3);
        assert_eq!(result.summary["samples_missing"], 4);
        assert_eq!(result.summary["samples_unchanged"], 0);
        assert_eq!(result.details[0]["before_nonfinite"], "NaN");
        for entities in [
            json!([null,{"NaN":[0],"Inf":[0]}]),
            json!([null,{"NaN":[4]}]),
            json!([null,{"Other":[0]}]),
        ] {
            frame["data"]["entities"] = entities;
            assert_eq!(
                run("prometheus", &response(vec![frame.clone()]), &data).err(),
                Some("unsupported_nonfinite_encoding")
            );
        }
    }

    #[test]
    fn sampling_intervals_must_match_each_series_not_only_the_set() {
        let mut a = metric(json!([1000]), json!([1]), "a");
        let mut b = metric(json!([1000]), json!([1]), "b");
        a["schema"]["fields"][0]["config"] = json!({"interval":1000});
        b["schema"]["fields"][0]["config"] = json!({"interval":2000});
        let before = response(vec![a.clone(), b.clone()]);
        a["schema"]["fields"][0]["config"] = json!({"interval":2000});
        b["schema"]["fields"][0]["config"] = json!({"interval":1000});
        let after = response(vec![a, b]);
        assert_eq!(
            run("prometheus", &before, &after).err(),
            Some("incompatible_sampling")
        );
    }

    #[test]
    fn sampling_metadata_gaps_and_phase_are_checked_without_interpolation() {
        let mut before = metric(json!([1000, 3000]), json!([1, 1]), "a");
        before["schema"]["fields"][0]["config"] = json!({"interval":1000});
        before["schema"]["meta"]["custom"]["calculatedMinStep"] = json!(1000);
        let data = response(vec![before.clone()]);
        let result = run("prometheus", &data, &data).unwrap();
        assert!(result.partial);
        assert_eq!(result.summary["before"]["sampling_gaps"], 1);
        assert!(result.reasons.contains(&"sampling_gaps"));
        assert_eq!(result.summary["samples_unchanged"], 2);
        before["schema"]["fields"][0]["config"]["interval"] = json!(2000);
        assert_eq!(
            run("prometheus", &data, &response(vec![before])).err(),
            Some("incompatible_sampling")
        );
        let shifted = response(vec![metric(json!([1500, 3500]), json!([1, 1]), "a")]);
        let ordinary = response(vec![metric(json!([1000, 3000]), json!([1, 1]), "a")]);
        assert_eq!(
            run("prometheus", &ordinary, &shifted).err(),
            Some("incompatible_sampling")
        );
    }

    #[test]
    fn actual_sampling_step_may_exceed_request_hint() {
        let mut frame = metric(json!([1000, 4000, 7000]), json!([1, 1, 0]), "a");
        frame["schema"]["fields"][0]["config"] = json!({"interval":3000});
        frame["schema"]["meta"]["custom"]["calculatedMinStep"] = json!(3000);
        let data = response(vec![frame]);
        let result = run("prometheus", &data, &data).unwrap();
        assert!(!result.partial);
        assert_eq!(result.summary["samples_unchanged"], 3);
        assert_eq!(
            result.summary["sampling"]["before_declared_intervals_ms"],
            json!([3000])
        );
    }

    #[test]
    fn offset_policy_retains_original_timestamps_and_submillisecond_precision() {
        let mut before = metric(json!([1000]), json!([1]), "a");
        before["data"]["nanos"] = json!([[123], null]);
        let mut after = before.clone();
        after["data"]["values"][0] = json!([11000]);
        after["data"]["values"][1] = json!([0]);
        let shifted = Window {
            from_ns: 10_000_000_000,
            to_ns: 20_000_000_000,
        };
        let result = compare(
            "prometheus",
            &response(vec![before]),
            &response(vec![after]),
            WINDOW,
            shifted,
            Normalization::WindowOffset,
            1000,
            10,
            10000,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(result.details[0]["timestamp_ns"], "1000000123");
        assert_eq!(result.details[0]["before_timestamp_ns"], "1000000123");
        assert_eq!(result.details[0]["after_timestamp_ns"], "11000000123");
    }

    #[test]
    fn duplicate_series_samples_outside_window_and_redacted_values_refuse() {
        let frame = metric(json!([1000]), json!([1]), "a");
        let data = response(vec![frame.clone()]);
        assert_eq!(
            run(
                "prometheus",
                &response(vec![frame.clone(), frame.clone()]),
                &data
            )
            .err(),
            Some("ambiguous_sample_identity")
        );
        let mut invalid = frame.clone();
        invalid["data"]["values"][0] = json!([11000]);
        assert_eq!(
            run("prometheus", &response(vec![invalid]), &data).err(),
            Some("observation_outside_window")
        );
        let mut invalid = frame.clone();
        invalid["schema"]["fields"][1]["labels"]["instance"] = json!("[REDACTED]");
        assert_eq!(
            run("prometheus", &response(vec![invalid]), &data).err(),
            Some("redacted_observation")
        );
        let mut invalid = data.clone();
        invalid["frames"][0]["schema"]["fields"][1]["config"] = json!({"redacted":true});
        assert_eq!(
            run("prometheus", &invalid, &data).err(),
            Some("redacted_observation")
        );
        invalid["frames"][0]["schema"]["fields"][1]["config"] = json!({"[REDACTED]":"none"});
        assert_eq!(
            run("prometheus", &invalid, &data).err(),
            Some("redacted_observation")
        );
        let mut invalid = frame;
        invalid["data"]["values"][1] = json!([{"redacted":true}]);
        assert_eq!(
            run("prometheus", &response(vec![invalid]), &data).err(),
            Some("redacted_observation")
        );
    }

    #[test]
    fn incompatible_units_profiles_columns_and_type_versions_refuse() {
        let frame = metric(json!([1000]), json!([1]), "a");
        let data = response(vec![frame.clone()]);
        let cases = [
            (
                "/schema/fields/1/config",
                json!({"unit":"percent"}),
                "incompatible_units",
            ),
            (
                "/schema/meta/type",
                json!("timeseries-long"),
                "unsupported_frame_profile",
            ),
            (
                "/schema/meta/typeVersion",
                json!([1, 0]),
                "unsupported_frame_profile",
            ),
            ("/data/values/1", json!([]), "unsupported_frame_shape"),
            (
                "/schema/fields/1/typeInfo/frame",
                json!("json.RawMessage"),
                "unsupported_frame_profile",
            ),
        ];
        for (pointer, value, error) in cases {
            let mut invalid = frame.clone();
            if pointer.ends_with("config") {
                invalid["schema"]["fields"][1]["config"] = value;
            } else {
                *invalid.pointer_mut(pointer).unwrap() = value;
            }
            assert_eq!(
                run("prometheus", &response(vec![invalid]), &data).err(),
                Some(error),
                "{pointer}"
            );
        }
    }

    #[test]
    fn recognized_empty_prometheus_frames_and_warning_notices_remain_partial() {
        let empty = response(vec![
            json!({"schema":{"fields":[],"meta":{"typeVersion":[0,0],"custom":{"calculatedMinStep":1000}}},"data":{"values":[]}}),
        ]);
        let result = run("prometheus", &empty, &empty).unwrap();
        assert!(result.partial);
        assert_eq!(result.summary["before"]["samples"], 0);
        let mut frame = metric(json!([1000]), json!([1]), "a");
        frame["schema"]["meta"]["notices"] = json!([{"severity":"warning","text":"partial query"}]);
        let data = response(vec![frame]);
        let result = run("prometheus", &data, &data).unwrap();
        assert!(result.reasons.contains(&"frame_notices"));
    }

    #[test]
    fn legacy_and_dataplane_logs_preserve_duplicate_multiplicity_and_nanoseconds() {
        let before = legacy_logs(
            vec![1000, 1000, 2000],
            vec![123, 123, 456],
            vec!["duplicate", "duplicate", "removed"],
        );
        let mut after = dataplane(legacy_logs(
            vec![1000, 3000],
            vec![123, 789],
            vec!["duplicate", "added"],
        ));
        after["data"]["values"][3] = json!(["different generated id", "[REDACTED]"]);
        let result = run("loki", &response(vec![before]), &response(vec![after])).unwrap();
        assert!(!result.partial);
        assert_eq!(result.summary["before"]["entries"], 3);
        assert_eq!(result.summary["entries_unchanged"], 1);
        assert_eq!(result.summary["entries_added"], 1);
        assert_eq!(result.summary["entries_removed"], 2);
        let duplicate = result
            .details
            .iter()
            .find(|event| event["line"] == "duplicate")
            .unwrap();
        assert_eq!(duplicate["count"], 1);
        assert_eq!(duplicate["before_count"], 2);
        assert_eq!(duplicate["timestamp_ns"], "1000000123");
    }

    #[test]
    fn log_timestamp_disagreement_and_conflicting_field_labels_refuse() {
        let frame = legacy_logs(vec![1000], vec![123], vec!["line"]);
        let data = response(vec![frame.clone()]);
        let mut invalid = frame.clone();
        invalid["data"]["values"][3][0] = json!("1000000124");
        assert_eq!(
            run("loki", &response(vec![invalid]), &data).err(),
            Some("inconsistent_timestamps")
        );
        let mut invalid = frame;
        invalid["schema"]["fields"][2]["labels"] = json!({"host":"other"});
        assert_eq!(
            run("loki", &response(vec![invalid]), &data).err(),
            Some("ambiguous_log_labels")
        );
    }

    #[test]
    fn legacy_nanoseconds_without_sdk_offsets_are_validated_against_milliseconds() {
        let mut legacy = legacy_logs(vec![1000], vec![123], vec!["line"]);
        legacy["data"].as_object_mut().unwrap().remove("nanos");
        let complete = response(vec![dataplane(legacy_logs(
            vec![1000],
            vec![123],
            vec!["line"],
        ))]);
        let result = run("loki", &response(vec![legacy.clone()]), &complete).unwrap();
        assert_eq!(result.summary["entries_unchanged"], 1);
        legacy["data"]["values"][3][0] = json!("1001000123");
        assert_eq!(
            run("loki", &response(vec![legacy]), &complete).err(),
            Some("inconsistent_timestamps")
        );
    }

    #[test]
    fn empty_log_sets_and_declared_truncation_have_distinct_completeness() {
        let empty = response(vec![]);
        let result = run("loki", &empty, &empty).unwrap();
        assert!(!result.partial);
        assert_eq!(result.summary["entries_unchanged"], 0);
        let mut frame = legacy_logs(vec![1000], vec![0], vec!["line"]);
        frame["schema"]["meta"]["custom"]["limit"] = json!(1);
        let data = response(vec![frame]);
        let result = run("loki", &data, &data).unwrap();
        assert!(result.partial);
        assert!(result.reasons.contains(&"source_truncated"));
    }

    #[test]
    fn log_label_classification_changes_are_not_erased() {
        let mut before = dataplane(legacy_logs(vec![1000], vec![123], vec!["line"]));
        before["schema"]["fields"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"labelTypes","type":"other"}));
        before["data"]["values"]
            .as_array_mut()
            .unwrap()
            .push(json!([{"job":"I","host":"I"}]));
        before["data"]["nanos"]
            .as_array_mut()
            .unwrap()
            .push(Value::Null);
        let mut after = before.clone();
        after["data"]["values"][4][0]["host"] = json!("P");
        let result = run(
            "loki",
            &response(vec![before.clone()]),
            &response(vec![after.clone()]),
        )
        .unwrap();
        assert_eq!(result.summary["entries_added"], 1);
        assert_eq!(result.summary["entries_removed"], 1);
        assert_eq!(result.summary["entries_unchanged"], 0);
        after["data"]["values"][4][0]["host"] = json!("unknown");
        assert_eq!(
            run("loki", &response(vec![before]), &response(vec![after])).err(),
            Some("unsupported_label_types")
        );
    }

    #[test]
    fn detail_bounds_keep_a_whole_deterministic_prefix_and_full_counts() {
        let before = response(vec![metric(
            json!([1000, 2000, 3000]),
            json!([0, 0, 0]),
            "a",
        )]);
        let after = response(vec![metric(
            json!([1000, 2000, 3000]),
            json!([1, 1, 1]),
            "a",
        )]);
        let result = compare(
            "prometheus",
            &before,
            &after,
            WINDOW,
            WINDOW,
            Normalization::ExactWindow,
            1000,
            1,
            10000,
            &mut || Ok(()),
        )
        .unwrap();
        assert_eq!(result.details.len(), 1);
        assert_eq!(result.omitted_details, 2);
        assert_eq!(result.summary["samples_changed"], 3);
        assert!(result.partial);
        let bytes = serde_json::to_vec(&result.details).unwrap().len();
        let bounded = compare(
            "prometheus",
            &before,
            &after,
            WINDOW,
            WINDOW,
            Normalization::ExactWindow,
            1000,
            100,
            bytes - 1,
            &mut || Ok(()),
        )
        .unwrap();
        assert!(bounded.details.is_empty());
        assert_eq!(bounded.omitted_details, 3);
    }

    #[test]
    fn cancellation_during_parse_and_oversized_observations_refuse() {
        let data = response(vec![metric(
            json!([1000, 2000, 3000]),
            json!([1, 1, 1]),
            "a",
        )]);
        let mut checks = 0;
        let result = compare(
            "prometheus",
            &data,
            &data,
            WINDOW,
            WINDOW,
            Normalization::ExactWindow,
            1000,
            100,
            10000,
            &mut || {
                checks += 1;
                if checks == 7 {
                    Err("cancelled")
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.err(), Some("cancelled"));
        let oversized = response(vec![metric(
            json!(vec![1000; 10_001]),
            json!(vec![1; 10_001]),
            "a",
        )]);
        assert_eq!(
            run("prometheus", &oversized, &data).err(),
            Some("frame_limit_exceeded")
        );
        let logs = response(vec![legacy_logs(
            vec![1000],
            vec![0],
            vec![&"a".repeat(MAX_LINE_BYTES + 1)],
        )]);
        assert_eq!(
            run("loki", &logs, &response(vec![])).err(),
            Some("frame_limit_exceeded")
        );
    }
}

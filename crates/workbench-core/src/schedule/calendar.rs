//! Pure occurrence planning against the bundled timezone database and an injected clock.
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::{GapInfo, Tz};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

pub(super) const TZDB_VERSION: &str = chrono_tz::IANA_TZDB_VERSION;
const NANOS_PER_SECOND: i128 = 1_000_000_000;
const MAX_UTC_NANOS: i128 = 253_402_300_800 * NANOS_PER_SECOND - 1;
const EPOCH_DAYS_FROM_CE: i64 = 719_163;
const LAST_UTC_DAY: i64 = 2_932_896;
const INVALID: &str = "invalid_schedule_cadence";
const RANGE: &str = "schedule_calendar_range";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Occurrence {
    pub ordinal: i64,
    pub due_utc: String,
    pub scheduled_utc: Option<String>,
    pub local: String,
    pub state: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SkippedRange {
    pub from_ordinal: i64,
    pub to_ordinal: i64,
    pub count: u64,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DueBatch {
    pub latest: Option<Occurrence>,
    pub skipped: Option<SkippedRange>,
    pub cursor: i64,
}

enum Cadence {
    Interval {
        anchor_nanos: i128,
        step_nanos: i128,
    },
    Daily {
        hour: u32,
        minute: u32,
    },
}
struct Calendar {
    cadence: Cadence,
    timezone: Tz,
}
struct RawOccurrence {
    ordinal: i64,
    due: DateTime<Utc>,
    scheduled: bool,
    local: String,
}
impl RawOccurrence {
    fn row(self) -> Result<Occurrence, &'static str> {
        utc_range(&self.due)?;
        let due_utc = self.due.to_rfc3339_opts(SecondsFormat::AutoSi, true);
        Ok(Occurrence {
            ordinal: self.ordinal,
            scheduled_utc: self.scheduled.then(|| due_utc.clone()),
            due_utc,
            local: self.local,
            state: if self.scheduled { "due" } else { "nonexistent" }.into(),
        })
    }
}

fn utc_range(value: &DateTime<Utc>) -> Result<(), &'static str> {
    if (1970..=9999).contains(&value.year()) && value.timestamp_subsec_nanos() < 1_000_000_000 {
        Ok(())
    } else {
        Err(RANGE)
    }
}
fn from_nanos(nanos: i128) -> Result<DateTime<Utc>, &'static str> {
    let seconds = i64::try_from(nanos.div_euclid(NANOS_PER_SECOND)).map_err(|_| RANGE)?;
    let fraction = nanos.rem_euclid(NANOS_PER_SECOND) as u32;
    DateTime::from_timestamp(seconds, fraction).ok_or(RANGE)
}
fn nanos(value: &DateTime<Utc>) -> i128 {
    i128::from(value.timestamp()) * NANOS_PER_SECOND + i128::from(value.timestamp_subsec_nanos())
}
fn clock(now: OffsetDateTime) -> Result<DateTime<Utc>, &'static str> {
    let now = from_nanos(now.unix_timestamp_nanos())?;
    utc_range(&now)?;
    Ok(now)
}
fn parse(cadence: &Value, timezone: &str) -> Result<Calendar, &'static str> {
    let timezone = timezone
        .parse::<Tz>()
        .map_err(|_| "invalid_schedule_timezone")?;
    let object = cadence.as_object().ok_or(INVALID)?;
    let cadence = match cadence["kind"].as_str() {
        Some("interval")
            if object.len() == 3
                && object.contains_key("anchor")
                && object.contains_key("every_seconds") =>
        {
            let anchor = cadence["anchor"].as_str().ok_or(INVALID)?;
            // Require explicit UTC and a four-digit Gregorian year. Do not turn
            // an offset spelling, leap second, or implicit local zone into authority.
            if !(anchor.len() == 20 || (22..=30).contains(&anchor.len()))
                || !anchor.ends_with('Z')
                || !anchor.as_bytes()[..4].iter().all(u8::is_ascii_digit)
                || anchor.as_bytes()[10] != b'T'
                || (anchor.len() > 20
                    && (anchor.as_bytes()[19] != b'.'
                        || !anchor.as_bytes()[20..anchor.len() - 1]
                            .iter()
                            .all(u8::is_ascii_digit)))
            {
                return Err(INVALID);
            }
            let anchor = DateTime::parse_from_rfc3339(anchor)
                .map_err(|_| INVALID)?
                .with_timezone(&Utc);
            utc_range(&anchor).map_err(|_| INVALID)?;
            let seconds = cadence["every_seconds"]
                .as_u64()
                .filter(|seconds| (60..=86_400).contains(seconds))
                .ok_or(INVALID)?;
            Cadence::Interval {
                anchor_nanos: nanos(&anchor),
                step_nanos: i128::from(seconds) * NANOS_PER_SECOND,
            }
        }
        Some("daily") if object.len() == 2 && object.contains_key("at") => {
            let at = cadence["at"].as_str().ok_or(INVALID)?.as_bytes();
            if at.len() != 5
                || at[2] != b':'
                || ![at[0], at[1], at[3], at[4]].iter().all(u8::is_ascii_digit)
            {
                return Err(INVALID);
            }
            let hour = u32::from(at[0] - b'0') * 10 + u32::from(at[1] - b'0');
            let minute = u32::from(at[3] - b'0') * 10 + u32::from(at[4] - b'0');
            if hour > 23 || minute > 59 {
                return Err(INVALID);
            }
            Cadence::Daily { hour, minute }
        }
        _ => return Err(INVALID),
    };
    Ok(Calendar { cadence, timezone })
}
impl Calendar {
    fn check_cursor(&self, after: i64) -> Result<(), &'static str> {
        let valid = match self.cadence {
            Cadence::Interval {
                anchor_nanos,
                step_nanos,
            } => after >= -1 && i128::from(after) <= (MAX_UTC_NANOS - anchor_nanos) / step_nanos,
            // Small sentinels allow a 1969 nominal local date whose real UTC
            // occurrence is in 1970, and a 10000 local date still in UTC 9999.
            Cadence::Daily { .. } => (-3..=LAST_UTC_DAY + 1).contains(&after),
        };
        if valid {
            Ok(())
        } else {
            Err("invalid_schedule_cursor")
        }
    }
    fn raw(&self, ordinal: i64) -> Result<RawOccurrence, &'static str> {
        match self.cadence {
            Cadence::Interval {
                anchor_nanos,
                step_nanos,
            } => {
                if ordinal < 0 {
                    return Err(RANGE);
                }
                let due = from_nanos(anchor_nanos + i128::from(ordinal) * step_nanos)?;
                let local = due
                    .with_timezone(&self.timezone)
                    .to_rfc3339_opts(SecondsFormat::AutoSi, false);
                Ok(RawOccurrence {
                    ordinal,
                    due,
                    scheduled: true,
                    local,
                })
            }
            Cadence::Daily { hour, minute } => {
                let ce = i32::try_from(ordinal.checked_add(EPOCH_DAYS_FROM_CE).ok_or(RANGE)?)
                    .map_err(|_| RANGE)?;
                let nominal = NaiveDate::from_num_days_from_ce_opt(ce)
                    .ok_or(RANGE)?
                    .and_hms_opt(hour, minute, 0)
                    .ok_or(RANGE)?;
                let resolved = match self.timezone.from_local_datetime(&nominal) {
                    LocalResult::Single(time) => Some(time),
                    LocalResult::Ambiguous(first, second) => Some(first.min(second)),
                    LocalResult::None => None,
                };
                if let Some(time) = resolved {
                    Ok(RawOccurrence {
                        ordinal,
                        due: time.with_timezone(&Utc),
                        scheduled: true,
                        local: time.to_rfc3339_opts(SecondsFormat::Secs, false),
                    })
                } else {
                    let end = GapInfo::new(&nominal, &self.timezone)
                        .and_then(|gap| gap.end)
                        .ok_or(RANGE)?;
                    Ok(RawOccurrence {
                        ordinal,
                        due: end.with_timezone(&Utc),
                        scheduled: false,
                        local: nominal.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    })
                }
            }
        }
    }
    fn latest_ordinal(&self, now: &DateTime<Utc>, inclusive: bool) -> Result<i64, &'static str> {
        match self.cadence {
            Cadence::Interval {
                anchor_nanos,
                step_nanos,
            } => {
                let difference = nanos(now) - anchor_nanos - i128::from(!inclusive);
                if difference < 0 {
                    Ok(-1)
                } else {
                    i64::try_from(difference / step_nanos).map_err(|_| RANGE)
                }
            }
            Cadence::Daily { .. } => {
                let today = i64::from(now.date_naive().num_days_from_ce()) - EPOCH_DAYS_FROM_CE;
                let mut latest = None;
                // FixedOffset is strictly within +/-24h. Both a resolved local
                // instant and a gap transition are within those offset bounds
                // of the nominal time. Five adjacent UTC dates therefore bound
                // this lookup, even for a complete skipped civil date.
                for ordinal in today - 2..=today + 2 {
                    let occurrence = self.raw(ordinal)?;
                    if occurrence.due < *now || (inclusive && occurrence.due == *now) {
                        latest = Some(ordinal);
                    }
                }
                latest.ok_or(RANGE)
            }
        }
    }
}

pub(super) fn validate(cadence: &Value, timezone: &str) -> Result<(), &'static str> {
    parse(cadence, timezone).map(|_| ())
}
pub(super) fn starting_cursor(
    cadence: &Value,
    timezone: &str,
    now: OffsetDateTime,
) -> Result<i64, &'static str> {
    parse(cadence, timezone)?.latest_ordinal(&clock(now)?, false)
}
pub(super) fn preview(
    cadence: &Value,
    timezone: &str,
    after: i64,
    count: usize,
) -> Result<Vec<Occurrence>, &'static str> {
    if !(1..=32).contains(&count) {
        return Err("invalid_schedule_preview_count");
    }
    let calendar = parse(cadence, timezone)?;
    calendar.check_cursor(after)?;
    (1..=count)
        .map(|offset| {
            calendar
                .raw(after.checked_add(offset as i64).ok_or(RANGE)?)?
                .row()
        })
        .collect()
}
pub(super) fn due(
    cadence: &Value,
    timezone: &str,
    after: i64,
    now: OffsetDateTime,
    grace_ms: u64,
) -> Result<DueBatch, &'static str> {
    if !(1..=60_000).contains(&grace_ms) {
        return Err("invalid_schedule_grace");
    }
    let calendar = parse(cadence, timezone)?;
    calendar.check_cursor(after)?;
    let now = clock(now)?;
    let ordinal = calendar.latest_ordinal(&now, true)?;
    if ordinal <= after {
        return Ok(DueBatch {
            latest: None,
            skipped: None,
            cursor: after,
        });
    }
    let occurrence = calendar.raw(ordinal)?;
    let eligible = nanos(&now) - nanos(&occurrence.due) <= i128::from(grace_ms) * 1_000_000;
    let end = ordinal - i64::from(eligible);
    let skipped = (end > after).then(|| SkippedRange {
        from_ordinal: after + 1,
        to_ordinal: end,
        count: (end - after) as u64,
        reason: "misfire".into(),
    });
    Ok(DueBatch {
        latest: if eligible {
            Some(occurrence.row()?)
        } else {
            None
        },
        skipped,
        cursor: ordinal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn now(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).unwrap()
    }
    fn daily(at: &str) -> Value {
        json!({"kind":"daily","at":at})
    }
    fn interval(anchor: &str) -> Value {
        json!({"kind":"interval","anchor":anchor,"every_seconds":60})
    }

    #[test]
    fn validation_refuses_implicit_time_unknown_fields_and_unbounded_cadences() {
        assert_eq!(TZDB_VERSION, "2025b");
        for value in [
            daily("24:00"),
            daily("1:30"),
            daily("01:60"),
            json!({"kind":"daily","at":"01:30","extra":true}),
            json!({"kind":"interval","anchor":"2026-01-01T00:00:00Z","every_seconds":59}),
            json!({"kind":"interval","anchor":"2026-01-01T00:00:00Z","every_seconds":86401}),
            interval("2026-01-01T00:00:00+00:00"),
            interval("2016-12-31T23:59:60Z"),
            interval("2026-01-01T00:00:00.1234567891Z"),
            interval("1969-12-31T23:59:59Z"),
        ] {
            assert!(validate(&value, "UTC").is_err(), "{value}");
        }
        assert!(validate(&daily("01:30"), "Not/A_Zone").is_err());
        assert!(validate(&json!(["daily", "01:30"]), "UTC").is_err());
        for seconds in [60, 86400] {
            assert!(validate(&json!({"kind":"interval","anchor":"2026-01-01T00:00:00Z","every_seconds":seconds}), "Asia/Kathmandu").is_ok());
        }
    }

    #[test]
    fn anchored_interval_preserves_subseconds_and_timezone_preview() {
        let cadence = interval("2026-01-01T00:00:00.125Z");
        let instant = now("2026-01-01T00:00:00.125Z");
        assert_eq!(
            starting_cursor(&cadence, "Asia/Kathmandu", instant).unwrap(),
            -1
        );
        let batch = due(&cadence, "Asia/Kathmandu", -1, instant, 1000).unwrap();
        assert_eq!(batch.cursor, 0);
        assert!(batch.skipped.is_none());
        let first = batch.latest.unwrap();
        assert_eq!(first.due_utc, "2026-01-01T00:00:00.125Z");
        assert_eq!(first.local, "2026-01-01T05:45:00.125+05:45");
        let next = preview(&cadence, "Asia/Kathmandu", 0, 1).unwrap();
        assert_eq!(next[0].due_utc, "2026-01-01T00:01:00.125Z");
        assert_eq!(next[0].ordinal, 1);
        assert!(
            due(&cadence, "UTC", -1, now("2025-12-31T23:59:59Z"), 1000)
                .unwrap()
                .latest
                .is_none()
        );
    }

    #[test]
    fn decades_of_misfires_are_compressed_and_backward_clock_never_rewinds_cursor() {
        let cadence = interval("1970-01-01T00:00:00Z");
        let batch = due(&cadence, "UTC", 0, now("2026-01-01T00:00:00Z"), 1000).unwrap();
        assert_eq!(batch.cursor, 29_453_760);
        assert_eq!(batch.latest.unwrap().due_utc, "2026-01-01T00:00:00Z");
        assert_eq!(
            batch.skipped.unwrap(),
            SkippedRange {
                from_ordinal: 1,
                to_ordinal: 29_453_759,
                count: 29_453_759,
                reason: "misfire".into()
            }
        );
        let backward = due(
            &cadence,
            "UTC",
            batch.cursor,
            now("1970-01-01T00:00:01Z"),
            1000,
        )
        .unwrap();
        assert_eq!(
            backward,
            DueBatch {
                latest: None,
                skipped: None,
                cursor: 29_453_760
            }
        );
    }

    #[test]
    fn grace_is_inclusive_and_stale_latest_is_also_skipped() {
        let cadence = interval("2026-01-01T00:00:00Z");
        assert!(
            due(&cadence, "UTC", -1, now("2026-01-01T00:00:01Z"), 1000)
                .unwrap()
                .latest
                .is_some()
        );
        let stale = due(&cadence, "UTC", -1, now("2026-01-01T00:00:01.001Z"), 1000).unwrap();
        assert!(stale.latest.is_none());
        assert_eq!(stale.skipped.unwrap().count, 1);
        assert_eq!(stale.cursor, 0);
        for grace in [0, 60001] {
            assert!(due(&cadence, "UTC", -1, now("2026-01-01T00:00:00Z"), grace).is_err());
        }
    }

    #[test]
    fn spring_gap_is_an_explicit_nonexecuting_occurrence() {
        let cadence = daily("02:30");
        let cursor =
            starting_cursor(&cadence, "America/New_York", now("2026-03-07T00:00:00Z")).unwrap();
        let rows = preview(&cadence, "America/New_York", cursor, 3).unwrap();
        assert_eq!(
            rows[0].scheduled_utc.as_deref(),
            Some("2026-03-07T07:30:00Z")
        );
        assert_eq!(rows[1].state, "nonexistent");
        assert_eq!(rows[1].due_utc, "2026-03-08T07:00:00Z");
        assert_eq!(rows[1].local, "2026-03-08T02:30:00");
        assert!(rows[1].scheduled_utc.is_none());
        assert_eq!(
            rows[2].scheduled_utc.as_deref(),
            Some("2026-03-09T06:30:00Z")
        );
        let batch = due(
            &cadence,
            "America/New_York",
            rows[0].ordinal,
            now("2026-03-08T07:00:00Z"),
            1000,
        )
        .unwrap();
        assert_eq!(batch.latest, Some(rows[1].clone()));
    }

    #[test]
    fn repeated_local_time_chooses_earlier_utc_once() {
        let cadence = daily("01:30");
        let cursor =
            starting_cursor(&cadence, "America/New_York", now("2026-10-31T00:00:00Z")).unwrap();
        let rows = preview(&cadence, "America/New_York", cursor, 3).unwrap();
        assert_eq!(
            rows[1].scheduled_utc.as_deref(),
            Some("2026-11-01T05:30:00Z")
        );
        assert_eq!(rows[1].local, "2026-11-01T01:30:00-04:00");
        assert_eq!(
            rows[2].scheduled_utc.as_deref(),
            Some("2026-11-02T06:30:00Z")
        );
        let first = due(
            &cadence,
            "America/New_York",
            rows[0].ordinal,
            now("2026-11-01T05:30:00Z"),
            1000,
        )
        .unwrap();
        let second = due(
            &cadence,
            "America/New_York",
            first.cursor,
            now("2026-11-01T06:30:00Z"),
            1000,
        )
        .unwrap();
        assert!(second.latest.is_none() && second.skipped.is_none());
        assert_eq!(
            starting_cursor(&cadence, "America/New_York", now("2026-11-01T06:00:00Z")).unwrap(),
            first.cursor
        );
    }

    #[test]
    fn half_hour_gap_uses_actual_transition_not_an_assumed_one_hour_shift() {
        let cadence = daily("02:15");
        let cursor =
            starting_cursor(&cadence, "Australia/Lord_Howe", now("2026-10-03T00:00:00Z")).unwrap();
        let row = preview(&cadence, "Australia/Lord_Howe", cursor, 1)
            .unwrap()
            .remove(0);
        assert_eq!(row.state, "nonexistent");
        assert_eq!(row.local, "2026-10-04T02:15:00");
        assert_eq!(row.due_utc, "2026-10-03T15:30:00Z");
    }

    #[test]
    fn skipped_civil_date_and_following_midnight_have_distinct_ordinals() {
        let cadence = daily("00:00");
        let cursor =
            starting_cursor(&cadence, "Pacific/Apia", now("2011-12-29T09:59:00Z")).unwrap();
        let rows = preview(&cadence, "Pacific/Apia", cursor, 3).unwrap();
        assert_eq!(rows[1].local, "2011-12-30T00:00:00");
        assert_eq!(rows[1].state, "nonexistent");
        assert_eq!(rows[1].due_utc, "2011-12-30T10:00:00Z");
        assert_eq!(rows[2].due_utc, rows[1].due_utc);
        assert_eq!(rows[2].ordinal, rows[1].ordinal + 1);
        assert_eq!(rows[2].state, "due");
        let instant = now("2011-12-30T10:00:00Z");
        assert_eq!(
            starting_cursor(&cadence, "Pacific/Apia", instant).unwrap(),
            rows[0].ordinal
        );
        let batch = due(&cadence, "Pacific/Apia", rows[0].ordinal, instant, 1000).unwrap();
        assert_eq!(batch.latest, Some(rows[2].clone()));
        assert_eq!(batch.skipped.unwrap().from_ordinal, rows[1].ordinal);
        assert_eq!(batch.cursor, rows[2].ordinal);
    }

    #[test]
    fn utc_range_edges_keep_first_real_occurrence_and_refuse_overflow() {
        let cadence = daily("23:00");
        let cursor =
            starting_cursor(&cadence, "America/Los_Angeles", now("1970-01-01T00:00:00Z")).unwrap();
        assert_eq!(cursor, -2);
        let row = preview(&cadence, "America/Los_Angeles", cursor, 1)
            .unwrap()
            .remove(0);
        assert_eq!(row.ordinal, -1);
        assert_eq!(row.local, "1969-12-31T23:00:00-08:00");
        assert_eq!(row.due_utc, "1970-01-01T07:00:00Z");
        let cadence = daily("23:59");
        let cursor = starting_cursor(&cadence, "UTC", now("9999-12-31T00:00:00Z")).unwrap();
        assert_eq!(
            preview(&cadence, "UTC", cursor, 1).unwrap()[0].due_utc,
            "9999-12-31T23:59:00Z"
        );
        assert!(preview(&cadence, "UTC", cursor, 2).is_err());
        assert!(starting_cursor(&cadence, "UTC", now("1969-12-31T23:59:59Z")).is_err());
        for after in [i64::MIN, i64::MAX] {
            assert!(preview(&cadence, "UTC", after, 1).is_err());
            assert!(
                due(
                    &interval("2026-01-01T00:00:00Z"),
                    "UTC",
                    after,
                    now("2026-01-01T00:00:00Z"),
                    1000
                )
                .is_err()
            );
        }
        for count in [0, 33] {
            assert!(preview(&cadence, "UTC", 0, count).is_err());
        }
    }
}

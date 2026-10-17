use super::*;
use std::collections::BTreeMap;
fn quantity(samples: &[&Sample], effort: bool) -> Value {
    let (mut known, mut measured, mut estimated, mut total) = (0_u64, 0_u64, 0_u64, 0_u64);
    for sample in samples {
        let (amount, kind) = if effort {
            (sample.effort_ms, &sample.effort_measurement)
        } else {
            (sample.elapsed_ms, &sample.elapsed_measurement)
        };
        if let Some(amount) = amount {
            known += 1;
            total += amount;
            if kind.as_deref() == Some("measured") {
                measured += 1;
            } else {
                estimated += 1;
            }
        }
    }
    json!({"known":known,"missing":samples.len() as u64-known,"measured":measured,"estimated":estimated,"total_ms":total,"mean_ms":if known>0{Some(total as f64/known as f64)}else{None}})
}
fn counter(samples: &[&Sample], manual: bool) -> Value {
    let (mut known, mut total) = (0_u64, 0_u64);
    for sample in samples {
        if let Some(n) = if manual {
            sample.manual_interventions
        } else {
            sample.retries
        } {
            known += 1;
            total += n;
        }
    }
    json!({"known":known,"missing":samples.len() as u64-known,"total":total,"mean":if known>0{Some(total as f64/known as f64)}else{None}})
}
fn cohort(samples: &[&Sample], window_ms: u64) -> Value {
    let mut outcomes: BTreeMap<&str, u64> = OUTCOMES.into_iter().map(|key| (key, 0)).collect();
    for sample in samples {
        *outcomes
            .get_mut(sample.outcome.as_str())
            .expect("validated outcome") += 1;
    }
    json!({"samples":samples.len(),"outcomes":outcomes,"frequency":{"runs_per_day":samples.len() as f64*86_400_000.0/window_ms as f64,"window_ms":window_ms},"elapsed":quantity(samples,false),"effort":quantity(samples,true),"retries":counter(samples,false),"manual_interventions":counter(samples,true)})
}
fn mul(a: i128, b: i128) -> Result<i128, &'static str> {
    a.checked_mul(b).ok_or("toil_arithmetic_limit")
}
fn sub(a: i128, b: i128) -> Result<i128, &'static str> {
    a.checked_sub(b).ok_or("toil_arithmetic_limit")
}
pub(super) fn build(
    input: &Value,
    samples: &[Sample],
    enabled: bool,
    retention_days: u64,
    now: OffsetDateTime,
    check: &mut dyn FnMut() -> Result<(), &'static str>,
) -> Result<Value, &'static str> {
    check()?;
    let from = utc(&input["from"]).expect("validated from");
    let to = utc(&input["to"]).expect("validated to");
    let window_ms =
        u64::try_from((to - from).whole_milliseconds()).map_err(|_| "toil_arithmetic_limit")?;
    let cutoff = now - time::Duration::days(retention_days as i64);
    let (mut baseline, mut candidate) = (Vec::new(), Vec::new());
    let mut expired = 0;
    for sample in samples {
        check()?;
        let observed = utc(&json!(sample.observed_at)).expect("validated observation");
        if sample.workflow != input["workflow"] || observed < from || observed >= to {
            continue;
        }
        if observed < cutoff {
            expired += 1;
            continue;
        }
        if sample.cohort == "baseline" {
            baseline.push(sample);
        } else {
            candidate.push(sample);
        }
    }
    let b = cohort(&baseline, window_ms);
    let c = cohort(&candidate, window_ms);
    let mut costs = input["costs"].clone();
    let total = costs["build_ms"].as_u64().unwrap()
        + costs["maintenance_ms"].as_u64().unwrap()
        + costs["incident_ms"].as_u64().unwrap();
    costs["total_ms"] = json!(total);
    let state = if baseline.is_empty() || candidate.is_empty() {
        "insufficient_samples"
    } else if b["effort"]["missing"] != 0 || c["effort"]["missing"] != 0 {
        "missing_effort"
    } else {
        "comparable"
    };
    let mut comparison = json!({"state":state,"basis":"counterfactual_comparison","input_measurement":null,"gross_per_run_ms":null,"observed_gross_ms":null,"observed_net_ms":null,"break_even_runs":null,"break_even_state":"unavailable"});
    let mut projection=input.get("projection").map(|p|json!({"measurement":"estimated","runs_per_day":p["runs_per_day"],"horizon_days":p["horizon_days"],"runs":p["runs_per_day"].as_u64().unwrap()*p["horizon_days"].as_u64().unwrap(),"gross_ms":null,"net_ms":null}));
    if state == "comparable" {
        let bn = baseline.len() as i128;
        let cn = candidate.len() as i128;
        let denominator = mul(bn, cn)?;
        let numerator = sub(
            mul(b["effort"]["total_ms"].as_u64().unwrap() as i128, cn)?,
            mul(c["effort"]["total_ms"].as_u64().unwrap() as i128, bn)?,
        )?;
        let cost_numerator = mul(total as i128, denominator)?;
        let observed = mul(numerator, cn)?;
        comparison["input_measurement"] = json!(if b["effort"]["estimated"] == 0
            && c["effort"]["estimated"] == 0
            && costs["measurement"] == "measured"
        {
            "measured"
        } else {
            "estimated"
        });
        comparison["gross_per_run_ms"] = json!(numerator as f64 / denominator as f64);
        comparison["observed_gross_ms"] = json!(observed as f64 / denominator as f64);
        comparison["observed_net_ms"] =
            json!(sub(observed, cost_numerator)? as f64 / denominator as f64);
        if numerator > 0 {
            let ceil = cost_numerator / numerator + i128::from(cost_numerator % numerator != 0);
            comparison["break_even_runs"] =
                json!(u64::try_from(ceil).map_err(|_| "toil_arithmetic_limit")?);
            comparison["break_even_state"] = json!("available");
        } else {
            comparison["break_even_state"] = json!("no_positive_benefit");
        }
        if let Some(p) = &mut projection {
            let gross = mul(numerator, p["runs"].as_u64().unwrap() as i128)?;
            p["gross_ms"] = json!(gross as f64 / denominator as f64);
            p["net_ms"] = json!(sub(gross, cost_numerator)? as f64 / denominator as f64);
        }
    }
    let mut limitations=vec!["Self-selected workflow episodes may be biased or describe unequal work. No causal benefit or employee ranking is established.".to_owned(),"All selected episodes, including failures and retries, contribute; elapsed runtime is not human labor. Gross/net savings compare cohort means counterfactually.".into(),"Frequency describes retained selected episodes within the explicit window, not a measured external arrival rate. Retention pruning is explicit; expired samples are excluded.".into()];
    if baseline.len() < 5 || candidate.len() < 5 {
        limitations.push("Small samples: fewer than five episodes in at least one cohort; do not treat the difference as a stable benefit estimate.".into());
    }
    if state != "comparable" {
        limitations.push("Labor savings are unavailable without at least one episode and known effort for every selected episode in both cohorts. Missing effort is not zero.".into());
    }
    if b["effort"]["estimated"] != 0
        || c["effort"]["estimated"] != 0
        || b["elapsed"]["estimated"] != 0
        || c["elapsed"]["estimated"] != 0
        || costs["measurement"] == "estimated"
    {
        limitations.push("Some durations or declared costs are estimates; measured and estimated input counts remain separate.".into());
    }
    if b["elapsed"]["missing"] != 0
        || c["elapsed"]["missing"] != 0
        || b["retries"]["missing"] != 0
        || c["retries"]["missing"] != 0
        || b["manual_interventions"]["missing"] != 0
        || c["manual_interventions"]["missing"] != 0
    {
        limitations.push("Some runtime, retry or intervention metadata is unknown; known-value means do not fill unknown values with zero.".into());
    }
    if projection.is_some() {
        limitations.push("Projected savings are estimates at operator-declared future volume and horizon, subtracting the same stated build, maintenance and incident costs once.".into());
    }
    check()?;
    Ok(
        json!({"format_version":1,"configuration":{"enabled":enabled,"retention_days":retention_days},"workflow":input["workflow"],"window":{"from":input["from"],"to":input["to"]},"cohorts":{"baseline":b,"candidate":c},"costs":costs,"comparison":comparison,"projection":projection,"expired_samples_excluded":expired,"limitations":limitations}),
    )
}

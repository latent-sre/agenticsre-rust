"""Pure structured adaptation of the pinned Save Toolkit error-budget calculator.

The original CLI remains in upstream.py for provenance. All quantities retain binary64
arithmetic and the original near-zero tolerance; this module does no I/O or printing.
"""
import math
from decimal import Decimal, localcontext

PAIRS = {("1h", "5m"): (14.4, "page"),
         ("6h", "30m"): (6.0, "page"),
         ("3d", "6h"): (1.0, "ticket")}
FIELDS = frozenset({"slo", "window_days", "bad_minutes", "bad_events", "total_events",
                    "sli_long", "sli_short", "long_window", "short_window"})


class CalculationError(Exception):
    def __init__(self, code):
        self.code = code


def require(condition):
    if not condition:
        raise CalculationError("invalid_calculation_input")


def numeric(value):
    require(type(value) in (int, float))
    try:
        result = float(value)
    except (ValueError, OverflowError):
        raise CalculationError("invalid_calculation_input") from None
    require(math.isfinite(result))
    return result


def normalize(inputs):
    require(isinstance(inputs, dict) and set(inputs) <= FIELDS and "slo" in inputs)
    result = dict(inputs)
    result.setdefault("window_days", 28.0)
    result.setdefault("long_window", "1h")
    result.setdefault("short_window", "5m")
    for key in FIELDS - {"long_window", "short_window"}:
        if key in result:
            result[key] = numeric(result[key])
    require(0 < result["slo"] < 100 and result["window_days"] > 0)
    require(type(result["long_window"]) is str and type(result["short_window"]) is str)
    require((result["long_window"], result["short_window"]) in PAIRS)
    for key in ("bad_minutes", "bad_events"):
        require(key not in result or result[key] >= 0)
    request_mode = "bad_events" in result or "total_events" in result
    if request_mode:
        require("bad_minutes" not in result and "bad_events" in result and "total_events" in result)
        require(result["total_events"] > 0 and result["bad_events"] <= result["total_events"])
    for key in ("sli_long", "sli_short"):
        require(key not in result or 0 <= result[key] <= 100)
    require("sli_short" not in result or "sli_long" in result)
    return result


def checked(value, *, positive=False):
    if not math.isfinite(value) or (positive and value <= 0):
        raise CalculationError("numeric_out_of_range")
    return value


def multiply(left, right):
    return checked(left * right, positive=left > 0 and right > 0)


def divide(left, right):
    if right <= 0:
        raise CalculationError("numeric_out_of_range")
    return checked(left / right, positive=left > 0)


def bad_fraction(percent):
    fraction = divide(percent, 100.0)
    result = checked(1.0 - fraction)
    if percent < 100 and result <= 0:
        raise CalculationError("numeric_out_of_range")
    return result


def status(budget, consumed, kind, observed):
    remaining = checked(budget - consumed)
    tolerance = max(abs(budget) * 1e-12, 1e-12)
    if abs(remaining) <= tolerance:
        state = "exhausted"
        remaining = 0.0
    elif remaining < 0:
        state = "over_budget"
    else:
        state = "ok"
    return {"kind": kind, "unit": "minutes" if kind == "time" else "events",
            "budget": budget, "consumed": consumed, "remaining": remaining,
            "consumed_percent": multiply(divide(consumed, budget), 100.0),
            "state": state, "observed_availability_percent": observed}


def at_threshold(sli, slo, threshold):
    # Narrow upstream correction: at SLO99/SLI85.6, binary64 displays a rate just
    # below 14.4. Decide from exact decimal forms of normalized input numbers,
    # without an epsilon that could page for a genuinely below-threshold value.
    # 400 digits covers every finite binary64 percentage's shortest decimal form.
    with localcontext() as context:
        context.prec = 400
        return (Decimal(100) - Decimal(str(sli))) >= Decimal(str(threshold)) * (Decimal(100) - Decimal(str(slo)))


def calculate(inputs):
    effective = normalize(inputs)
    allowed = bad_fraction(effective["slo"])
    calculation = {"allowed_bad_fraction": allowed, "status": None, "burn": None}
    if "bad_minutes" in effective:
        window_minutes = multiply(multiply(effective["window_days"], 24.0), 60.0)
        budget = multiply(window_minutes, allowed)
        calculation["status"] = status(budget, effective["bad_minutes"], "time", None)
    elif "bad_events" in effective:
        budget = multiply(effective["total_events"], allowed)
        observed = multiply(checked(1.0 - divide(effective["bad_events"], effective["total_events"])), 100.0)
        calculation["status"] = status(budget, effective["bad_events"], "request", observed)
    if "sli_long" in effective:
        threshold, action = PAIRS[(effective["long_window"], effective["short_window"])]
        long_rate = divide(bad_fraction(effective["sli_long"]), allowed)
        burn = {"policy": "fixed-30-day-example-v1", "policy_horizon_days": 30,
                "long_window": effective["long_window"], "short_window": effective["short_window"],
                "threshold": threshold, "long_rate": long_rate, "short_rate": None,
                "severity": "not_evaluated", "reason": "short_window_missing",
                "full_budget_projection_days": None, "projection_state": "not_evaluated"}
        if "sli_short" in effective:
            short_rate = divide(bad_fraction(effective["sli_short"]), allowed)
            burn["short_rate"] = short_rate
            long_crossed = at_threshold(effective["sli_long"], effective["slo"], threshold)
            short_crossed = at_threshold(effective["sli_short"], effective["slo"], threshold)
            if long_crossed and short_crossed:
                burn["severity"], burn["reason"] = action, "both_windows_at_threshold"
            elif long_crossed:
                burn["severity"], burn["reason"] = "none", "short_window_recovered"
            elif short_crossed:
                burn["severity"], burn["reason"] = "none", "long_window_unconfirmed"
            else:
                burn["severity"], burn["reason"] = "none", "both_below_threshold"
            if long_rate > 0:
                burn["full_budget_projection_days"] = divide(effective["window_days"], long_rate)
                burn["projection_state"] = "estimated"
            else:
                burn["projection_state"] = "zero_long_burn"
        calculation["burn"] = burn
    return {"input": effective, "calculation": calculation}

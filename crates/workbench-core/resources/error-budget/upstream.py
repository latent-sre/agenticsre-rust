#!/usr/bin/env python3
"""Error-budget and multi-window burn-rate calculator.

Pure stdlib, cross-platform.

Two SLI units are supported and never mixed:

* Time-based status (``--bad-minutes``) measures a probe/uptime SLI. Its budget is minutes.
* Request-based status (``--bad-events`` and ``--total-events``) measures a request-ratio SLI.
  Its budget is a count of failed requests; converting it to downtime assumes uniform traffic.

Burn-rate severity requires two measurements and one of three bound long/short window pairs. A
single window can show arithmetic but cannot emit PAGE or TICKET.
"""

import argparse
import math
import sys


def fmt_minutes(minutes: float) -> str:
    """Format a minute quantity without changing its unit semantics."""
    if minutes >= 1440:
        return f"{minutes / 1440:.2f} d ({minutes:.0f} min)"
    if minutes >= 60:
        return f"{minutes / 60:.2f} h ({minutes:.0f} min)"
    return f"{minutes:.1f} min"


def fmt_count(count: float) -> str:
    """Format a request count."""
    return f"{count:,.0f}"


def _budget_state(remaining: float, budget: float) -> str:
    """Classify remaining budget with tolerance for percentage floating-point arithmetic."""
    tolerance = max(abs(budget) * 1e-12, 1e-12)
    if abs(remaining) <= tolerance:
        return "EXHAUSTED"
    if remaining < 0:
        return "OVER BUDGET"
    return "ok"


def _display_remaining(remaining: float, budget: float) -> float:
    """Clamp floating-point zero so exhausted budgets never print as ``-0.0``."""
    tolerance = max(abs(budget) * 1e-12, 1e-12)
    return 0.0 if abs(remaining) <= tolerance else remaining


def _finite(parser, name, value, *, minimum=None, exclusive_min=None, maximum=None):
    """Reject NaN/inf and enforce a numeric argument's range."""
    if value is None:
        return None
    if not math.isfinite(value):
        parser.error(f"{name} must be a finite number (got {value!r})")
    if minimum is not None and value < minimum:
        parser.error(f"{name} must be >= {minimum} (got {value:g})")
    if exclusive_min is not None and value <= exclusive_min:
        parser.error(f"{name} must be > {exclusive_min} (got {value:g})")
    if maximum is not None and value > maximum:
        parser.error(f"{name} must be <= {maximum} (got {value:g})")
    return value


def main(argv=None) -> int:
    """Run the calculator and return a process exit code."""
    parser = argparse.ArgumentParser(
        description="SLO error-budget and burn-rate calculator",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--slo", type=float, required=True, help="SLO target percent, e.g. 99.9")
    parser.add_argument(
        "--window-days", type=float, default=28.0,
        help="budget-status horizon in days (default 28); does not rescale fixed alert thresholds",
    )

    status = parser.add_argument_group("budget status — pick ONE kind of SLI")
    status.add_argument(
        "--bad-minutes", type=float,
        help="TIME-based: bad minutes consumed in the budget-status window",
    )
    status.add_argument(
        "--bad-events", type=float,
        help="REQUEST-based: failed requests in the window (needs --total-events)",
    )
    status.add_argument(
        "--total-events", type=float,
        help="REQUEST-based: total valid requests in the window",
    )

    burn = parser.add_argument_group("burn rate — BOTH windows required for a severity verdict")
    burn.add_argument(
        "--sli-long", "--sli", dest="sli_long", type=float,
        help="availability percent measured over the selected long window",
    )
    burn.add_argument(
        "--sli-short", type=float,
        help="availability percent measured over the selected short window",
    )
    burn.add_argument(
        "--long-window", default="1h", choices=["1h", "6h", "3d"],
        help="long window of the pair; selects the alert threshold (1h/5m=14.4x page, "
             "6h/30m=6x page, 3d/6h=1x ticket)",
    )
    burn.add_argument(
        "--short-window", default="5m", choices=["5m", "30m", "6h"],
        help="short window of the pair; must match the long window's pair",
    )

    args = parser.parse_args(argv)

    _WINDOW_PAIRS = {  # Google SRE Workbook: threshold and action selected by the pair.
        ("1h", "5m"): (14.4, "PAGE (fast burn)"),
        ("6h", "30m"): (6.0, "PAGE (slow burn)"),
        ("3d", "6h"): (1.0, "TICKET (slow leak)"),
    }
    pair = (args.long_window, args.short_window)
    if pair not in _WINDOW_PAIRS:
        parser.error(
            "--long-window/--short-window must be one of: "
            + ", ".join("%s/%s" % key for key in _WINDOW_PAIRS)
        )

    _finite(parser, "--slo", args.slo, exclusive_min=0, maximum=100)
    if args.slo >= 100:
        parser.error("--slo must be < 100 (a 100% SLO has a zero error budget)")
    _finite(parser, "--window-days", args.window_days, exclusive_min=0)
    _finite(parser, "--bad-minutes", args.bad_minutes, minimum=0)
    _finite(parser, "--bad-events", args.bad_events, minimum=0)
    _finite(parser, "--total-events", args.total_events, exclusive_min=0)
    _finite(parser, "--sli-long", args.sli_long, minimum=0, maximum=100)
    _finite(parser, "--sli-short", args.sli_short, minimum=0, maximum=100)

    request_mode = args.bad_events is not None or args.total_events is not None
    if request_mode and args.bad_minutes is not None:
        parser.error(
            "--bad-minutes (time-based SLI) cannot be combined with "
            "--bad-events/--total-events (request-based SLI); pick one unit"
        )
    if request_mode and (args.bad_events is None or args.total_events is None):
        parser.error("request-based status needs BOTH --bad-events and --total-events")
    if request_mode and args.bad_events > args.total_events:
        parser.error("--bad-events cannot exceed --total-events")
    if args.sli_short is not None and args.sli_long is None:
        parser.error("--sli-short requires --sli-long")

    budget_fraction = 1.0 - args.slo / 100.0
    print(f"SLO {args.slo}%  ->  error budget = {budget_fraction * 100:.4g}% of the SLI")

    # Budget status: the unit follows the SLI.
    if args.bad_minutes is not None:
        window_minutes = args.window_days * 24 * 60
        budget_minutes = window_minutes * budget_fraction
        remaining = budget_minutes - args.bad_minutes
        percent = args.bad_minutes / budget_minutes * 100
        state = _budget_state(remaining, budget_minutes)
        remaining = _display_remaining(remaining, budget_minutes)
        print(
            f"  [time-based SLI] over {args.window_days:g}d the budget is "
            f"{fmt_minutes(budget_minutes)}"
        )
        print(
            f"  consumed:  {fmt_minutes(args.bad_minutes)}  "
            f"({percent:.1f}% of budget)  [{state}]"
        )
        print(f"  remaining: {fmt_minutes(remaining)}")

    if request_mode:
        budget_events = args.total_events * budget_fraction
        remaining = budget_events - args.bad_events
        percent = args.bad_events / budget_events * 100 if budget_events else float("inf")
        state = _budget_state(remaining, budget_events)
        remaining = _display_remaining(remaining, budget_events)
        observed = (1 - args.bad_events / args.total_events) * 100
        print(
            f"  [request-based SLI] {fmt_count(args.total_events)} requests  ->  "
            f"budget = {fmt_count(budget_events)} failed requests"
        )
        print(
            f"  consumed:  {fmt_count(args.bad_events)} bad  "
            f"({percent:.1f}% of budget)  [{state}]"
        )
        print(f"  remaining: {fmt_count(remaining)} bad requests")
        print(f"  observed availability: {observed:.4f}%")

    # Burn rate: a pair selects one threshold, and both windows must cross it.
    if args.sli_long is not None:
        print("  alert policy: fixed 30-day example thresholds; --window-days does not rescale them")
        burn_long = (1.0 - args.sli_long / 100.0) / budget_fraction
        print(f"  burn ({args.long_window}):  SLI {args.sli_long}%  ->  {burn_long:.2f}x")

        if args.sli_short is None:
            print(
                "  severity: NOT EVALUATED -- pass --sli-short; one window cannot emit "
                "PAGE or TICKET"
            )
        else:
            burn_short = (1.0 - args.sli_short / 100.0) / budget_fraction
            print(
                f"  burn ({args.short_window}): SLI {args.sli_short}%  ->  "
                f"{burn_short:.2f}x"
            )
            threshold, verdict = _WINDOW_PAIRS[pair]
            both = min(burn_long, burn_short)  # BOTH windows must exceed the pair's threshold.
            if both >= threshold:
                severity = "%s -- both windows >= %sx" % (verdict, threshold)
            elif burn_long >= threshold:
                severity = (
                    "no page -- long window at %.2fx but the short window (%.2fx) has recovered. "
                    "NOT an all-clear: budget status is unknown; some budget may already have been "
                    "consumed. Run the budget-status mode."
                    % (burn_long, burn_short)
                )
            elif burn_short >= threshold:
                severity = (
                    "no page -- short-window spike (%.2fx) the long window (%.2fx) hasn't "
                    "confirmed. Re-check in minutes; a real burn trips both."
                    % (burn_short, burn_long)
                )
            else:
                severity = (
                    "below the %sx threshold for the %s/%s pair. This says nothing about the "
                    "budget already consumed -- that is the budget-status mode's job."
                    % (threshold, args.long_window, args.short_window)
                )
            print(f"  severity: {severity}")

            if burn_long > 0:
                exhaustion_days = args.window_days / burn_long
                print(
                    f"  at the {args.long_window} rate, the full {args.window_days:g}d budget "
                    f"is gone in {fmt_minutes(exhaustion_days * 24 * 60)} "
                    "(projection assumes steady eligible volume; not remaining budget)"
                )

    if args.bad_minutes is None and not request_mode and args.sli_long is None:
        print(
            "  (pass --bad-minutes OR --bad-events/--total-events for status; "
            "--sli-long + --sli-short for burn rate)"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())

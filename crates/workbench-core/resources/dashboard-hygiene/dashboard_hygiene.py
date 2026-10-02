#!/usr/bin/env python3
"""Dashboard hygiene checker — the mechanically checkable subset of this skill's panel rules.

Pure stdlib and offline. Checks textual properties only: no query parsing, datasource resolution,
or Grafana access. A clean result means no implemented rule fired, not that the dashboard is correct.
PromQL heuristics run only for a resolved model reference whose type is `prometheus`; other or
untyped datasource references need their dialect's query validation. Findings need human review:
fixed UIDs can be intentional in provisioned dashboards, and a `_total` suffix is not type evidence.
Prefer dashboard-linter where installed for real query/schema validation; this helper is the
portable fallback for Classic/V1 models.

Which model shapes it accepts:
  * Classic / V1 `spec`  — the `panels[]` + `templating.list[]` shape
  * an app-platform wrapper — `{apiVersion, kind, metadata, spec}`; the `spec` is unwrapped
  * V2 (`elements`/`layout`) is DETECTED AND REFUSED rather than half-checked: its panels live in a
    different structure, and silently checking zero panels would be worse than saying so.

Exit codes: 0 clean, 1 violations found, 2 the input could not be checked.
"""

from __future__ import annotations

import argparse
import json
import re
import sys

# Panel types for which a unit is meaningful. A `text` or `row` panel has no unit, and flagging one
# would train people to ignore the checker.
UNIT_BEARING = frozenset({"timeseries", "stat", "gauge", "bargauge", "table", "barchart", "trend"})
# Panels that render without querying. Demanding a target or a "no value" string from these reports
# the very documentation panel this skill tells you to add, which trains people to ignore the
# checker as surely as a false unit warning would.
NON_QUERYING = frozenset({"text", "dashlist", "news", "welcome", "alertlist"})
# Range-vector functions whose window must adapt to the panel's resolution.
RATE_FUNCS = re.compile(r"\b(rate|irate|increase)\s*\(")
COUNTER_NAME = re.compile(r"\b(\w+_total)\b")
RANGE_SELECTOR = re.compile(r"\[([^\]]*)\]")
# Ignore string contents without shifting spans; regex label values may contain brackets,
# parentheses, counter-like names, or text resembling functions and Grafana macros.
QUOTED_STRING = re.compile(r'''"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`[^`]*`''')
# A datasource reference that is a literal uid rather than a variable.
BUILTIN_DS = frozenset({"-- Grafana --", "-- Mixed --", "-- Dashboard --", "grafana"})


def rate_call_spans(expr):
    """[(start, end)] for each rate/irate/increase call, end just past its closing paren.

    Span-based, because both rules that use it are per-call and the string-level shortcuts are
    wrong in opposite directions:

    * "does the expression contain $__rate_interval anywhere" passes
      `rate(a_total[$__rate_interval]) + rate(b_total[5m])`, whose second call is exactly what the
      rule exists to reject;
    * "count parentheses before the metric" decides `sum(rate(a[$__rate_interval])) / sum(b_total)`
      has `b_total` inside a rate call, because it cannot see that the earlier call already closed.
    """
    spans = []
    for match in RATE_FUNCS.finditer(expr):
        depth = 0
        index = match.end() - 1  # the opening paren
        while index < len(expr):
            if expr[index] == "(":
                depth += 1
            elif expr[index] == ")":
                depth -= 1
                if depth == 0:
                    spans.append((match.start(), index + 1))
                    break
            index += 1
        else:
            # Unbalanced input: treat the remainder as the call rather than dropping it, so a
            # malformed query is still inspected instead of silently passing.
            spans.append((match.start(), len(expr)))
    return spans


def unwrap(model: dict) -> dict:
    """Return the dashboard spec whether the caller passed a bare model or a k8s wrapper."""
    if "spec" in model and "apiVersion" in model:
        return model["spec"] or {}
    # a legacy GET returns {dashboard, meta}
    if "dashboard" in model and "meta" in model:
        return model["dashboard"] or {}
    return model


def iter_panels(spec: dict):
    """Yield every non-row panel, descending into collapsed rows."""
    def walk(panels):
        for panel in panels or []:
            if panel.get("type") == "row":
                yield from walk(panel.get("panels") or [])
            else:
                yield panel
    yield from walk(spec.get("panels"))


def check(spec: dict) -> list[tuple[str, str, str]]:
    """Return [(rule, where, detail)] for every violation found."""
    out: list[tuple[str, str, str]] = []

    for panel in iter_panels(spec):
        title = (panel.get("title") or "").strip()
        where = title or f"panel id={panel.get('id')}"
        defaults = (panel.get("fieldConfig") or {}).get("defaults") or {}
        ptype = panel.get("type")

        querying = ptype not in NON_QUERYING

        if not title:
            out.append(("panel-title", where, "panel has no title; a title states the question it answers"))
        if not (panel.get("description") or "").strip():
            out.append(("panel-description", where, "no description; it renders as the panel's tooltip"))
        if ptype in UNIT_BEARING and not defaults.get("unit"):
            out.append(("panel-units", where, f"{ptype} panel has no unit set"))
        if querying and not (panel.get("targets") or []):
            out.append(("panel-no-targets", where, "panel has no query"))
        if querying and "noValue" not in defaults:
            out.append(("panel-no-value", where, "no 'No value' text; an empty panel must not read as healthy"))

        # Portability is decided per datasource reference, and a target may override the panel's.
        # Checking only the panel level passes a dashboard whose panel says ${datasource} while one
        # target names a uid -- Grafana uses the target's, so the model breaks on another instance.
        references = [("panel", panel.get("datasource"))]
        references += [(f"target {t.get('refId', '?')}", t.get("datasource"))
                       for t in panel.get("targets") or []]
        for scope, datasource in references:
            if not isinstance(datasource, dict):
                continue
            uid = datasource.get("uid")
            if isinstance(uid, str) and not uid.startswith("$") and uid not in BUILTIN_DS:
                out.append(("panel-datasource", where,
                            f"{scope} names a hard-coded data-source uid {uid!r}; use a datasource "
                            f"variable so the model is portable"))

        for target in panel.get("targets") or []:
            datasource = target.get("datasource") or panel.get("datasource") or {}
            if not isinstance(datasource, dict) or datasource.get("type") != "prometheus":
                continue
            expr = target.get("expr") or target.get("query") or ""
            if not isinstance(expr, str) or not expr:
                continue
            ref = target.get("refId", "?")
            query_code = QUOTED_STRING.sub(lambda match: " " * len(match.group()), expr)
            spans = rate_call_spans(query_code)
            for start, end in spans:
                call = expr[start:end]
                windows = RANGE_SELECTOR.findall(query_code[start:end])
                selected_total = (re.match(r"increase\s*\(", call) is not None
                                  and windows and all(w.strip() == "$__range" for w in windows))
                if windows and not selected_total and not any("$__rate_interval" in w for w in windows):
                    out.append(("target-rate-interval", f"{where} [{ref}]",
                                f"{call[:60]!r} uses a fixed or non-rate window; review the intended "
                                f"horizon and scrape cadence before choosing $__rate_interval"))
            for match in COUNTER_NAME.finditer(query_code):
                if not any(start <= match.start() < end for start, end in spans):
                    out.append(("target-counter-agg", f"{where} [{ref}]",
                                f"counter-like name {match.group(1)!r} is outside rate/irate/increase; "
                                f"verify metric type and intent (presence, counts, resets, and lifetime "
                                f"totals can legitimately read it directly)"))
                    break

    for variable in (spec.get("templating") or {}).get("list") or []:
        name = variable.get("name", "?")
        if variable.get("includeAll") and not (variable.get("allValue") or "").strip():
            out.append(("template-all-value", f"${name}",
                        "'Include All' with no custom all value; the expanded expression can grow "
                        "unbounded — set one such as '.+'"))

    # Tags support discovery regardless of whether the dashboard is provisioned or UI-managed.
    if not (spec.get("tags") or []):
        out.append(("dashboard-tags", "dashboard", "no tags; search and the dashboard list rely on them"))

    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("path", help="dashboard JSON: a bare model, a k8s wrapper, or a legacy GET body")
    parser.add_argument("--quiet", action="store_true", help="print only the summary line")
    args = parser.parse_args(argv)

    try:
        with open(args.path, encoding="utf-8") as handle:
            model = json.load(handle)
    except (OSError, ValueError) as exc:
        print(f"cannot check {args.path}: {exc}", file=sys.stderr)
        return 2

    spec = unwrap(model)
    if "elements" in spec or "layout" in spec:
        print("refusing to check a V2 (dynamic) dashboard: its panels live under spec.elements, which "
              "this checker does not read. Validate the actual V2 candidate with a V2-capable linter "
              "(dashboard-linter v0.2.0+), or report the validation gap; keep its stored version.", file=sys.stderr)
        return 2
    if "panels" not in spec:
        print("no `panels` key: this does not look like a Classic/V1 dashboard model", file=sys.stderr)
        return 2

    violations = check(spec)
    panel_count = sum(1 for _ in iter_panels(spec))
    if not args.quiet:
        for rule, where, detail in violations:
            print(f"{rule}: {where}\n    {detail}")
        if violations:
            print()
    title = spec.get("title", "<untitled>")
    print(f"{title!r}: {len(violations)} violation(s) across {panel_count} panel(s)")
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())

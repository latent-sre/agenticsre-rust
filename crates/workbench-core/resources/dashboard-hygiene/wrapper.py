"""Fixed structured boundary; receives a literal path, never caller-supplied source."""
import hashlib
import json
import os
import stat
import sys

CHECKER_SOURCE = __WORKBENCH_CHECKER_SOURCE__
MAX_BYTES = 2 * 1024 * 1024
MAX_RESPONSE = 512 * 1024


class InvalidModel(Exception):
    def __init__(self, code):
        self.code = code


def require(condition, code="invalid_model"):
    if not condition:
        raise InvalidModel(code)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_model_key")
        result[key] = value
    return result


def check_depth(text):
    depth = 0
    quoted = False
    escaped = False
    for char in text:
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in "[{":
            depth += 1
            require(depth <= 32, "model_too_deep")
        elif char in "]}":
            depth -= 1


def string_fields(value, keys):
    for key in keys:
        if key in value:
            require(isinstance(value[key], str))


def datasource(value):
    require(value is None or isinstance(value, (str, dict)))
    if isinstance(value, dict):
        string_fields(value, ("uid", "type"))


def validate(spec):
    require(isinstance(spec, dict))
    require("elements" not in spec and "layout" not in spec, "unsupported_model_v2")
    require(isinstance(spec.get("panels"), list))
    string_fields(spec, ("title",))
    if "tags" in spec:
        require(isinstance(spec["tags"], list) and all(isinstance(tag, str) for tag in spec["tags"]))
    variables = []
    if "templating" in spec:
        require(isinstance(spec["templating"], dict))
        variables = spec["templating"].get("list", [])
        require(isinstance(variables, list))
    fields_truncated = False
    for variable in variables:
        require(isinstance(variable, dict))
        string_fields(variable, ("name", "allValue"))
        if "includeAll" in variable:
            require(isinstance(variable["includeAll"], bool))
        if len(variable.get("name", "")) > 128:
            variable["name"] = variable["name"][:128]
            fields_truncated = True
    panels = []
    pending = list(reversed(spec["panels"]))
    while pending:
        panel = pending.pop()
        require(isinstance(panel, dict) and isinstance(panel.get("type"), str) and bool(panel["type"]))
        string_fields(panel, ("title", "description"))
        if "id" in panel:
            require(isinstance(panel["id"], int) and not isinstance(panel["id"], bool))
        if "panels" in panel:
            require(isinstance(panel["panels"], list))
        if panel["type"] == "row":
            pending.extend(reversed(panel.get("panels", [])))
            continue
        panels.append(panel)
        require(len(panels) <= 1000, "too_many_panels")
        if "fieldConfig" in panel:
            require(isinstance(panel["fieldConfig"], dict))
            defaults = panel["fieldConfig"].get("defaults", {})
            require(isinstance(defaults, dict))
            string_fields(defaults, ("unit", "noValue"))
        datasource(panel.get("datasource"))
        targets = panel.get("targets", [])
        require(isinstance(targets, list))
        for target in targets:
            require(isinstance(target, dict))
            string_fields(target, ("expr", "query", "refId"))
            require(len(target.get("expr", "")) <= 16000 and len(target.get("query", "")) <= 16000, "query_too_large")
            datasource(target.get("datasource"))
            if len(target.get("refId", "")) > 128:
                target["refId"] = target["refId"][:128]
                fields_truncated = True
        # Only display labels are shortened before the checker to prevent a huge title
        # being amplified by many findings. Rule decisions and query text are unchanged.
        if len(panel.get("title", "").strip()) > 512:
            panel["title"] = panel["title"].strip()[:512]
            fields_truncated = True
    require(bool(panels), "uncheckable_model")
    return panels, fields_truncated


def encoded(value):
    return json.dumps(value, ensure_ascii=True, separators=(",", ":")).encode("ascii")


def main():
    reply = {"task": {"id": "dashboard-hygiene", "version": 1}, "status": "error",
             "runtime_version": ".".join(map(str, sys.version_info[:3])), "input_digest": None,
             "input_bytes": 0, "checked_panels": 0, "findings": [], "findings_total": 0,
             "findings_truncated": False, "fields_truncated": False, "error_code": None}
    try:
        require(sys.version_info >= (3, 11), "unsupported_runtime")
        require(len(sys.argv) == 2 and os.path.isabs(sys.argv[1]), "invalid_task_input")
        descriptor = os.open(sys.argv[1], os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
        with os.fdopen(descriptor, "rb") as handle:
            require(stat.S_ISREG(os.fstat(handle.fileno()).st_mode), "non_regular_model")
            content = handle.read(MAX_BYTES + 1)
        require(len(content) <= MAX_BYTES, "model_too_large")
        reply["input_bytes"] = len(content)
        reply["input_digest"] = "sha256:" + hashlib.sha256(content).hexdigest()
        text = content.decode("utf-8")
        check_depth(text)
        def invalid_constant(_value):
            raise InvalidModel("invalid_model_json")
        model = json.loads(text, object_pairs_hook=unique_object, parse_constant=invalid_constant)
        require(isinstance(model, dict))
        if "spec" in model and "apiVersion" in model:
            require(isinstance(model["spec"], dict) and isinstance(model["apiVersion"], str))
        if "dashboard" in model and "meta" in model:
            require(isinstance(model["dashboard"], dict) and isinstance(model["meta"], dict))
        namespace = {"__name__": "workbench_dashboard_hygiene"}
        exec(compile(CHECKER_SOURCE, "<embedded-dashboard-hygiene>", "exec"), namespace)
        spec = namespace["unwrap"](model)
        panels, shortened = validate(spec)
        findings = namespace["check"](spec)
        reply["checked_panels"] = len(panels)
        reply["findings_total"] = len(findings)
        reply["fields_truncated"] = shortened
        # Reserve envelope fields inside the child budget, independently of the caller's
        # capture budget. Rust validates the whole child protocol before accepting it.
        used = len(encoded(reply)) + 1024
        for rule, location, detail in findings:
            item = {"rule": rule, "location": location[:512], "detail": detail[:512]}
            if len(location) > 512 or len(detail) > 512:
                reply["fields_truncated"] = True
            item_bytes = len(encoded(item)) + 1
            if len(reply["findings"]) >= 1000 or used + item_bytes > MAX_RESPONSE:
                reply["findings_truncated"] = True
                break
            reply["findings"].append(item)
            used += item_bytes
        reply["status"] = "partial" if reply["fields_truncated"] or reply["findings_truncated"] else "complete"
    except InvalidModel as error:
        reply["error_code"] = error.code
    except OSError:
        reply["error_code"] = "model_unreadable"
    except UnicodeError:
        reply["error_code"] = "invalid_model_encoding"
    except (ValueError, RecursionError):
        reply["error_code"] = "invalid_model_json"
    except Exception:
        # Task boundary: never emit arbitrary input, exception bodies or traceback data.
        reply["error_code"] = "task_internal_error"
    sys.stdout.buffer.write(encoded(reply) + b"\n")


if __name__ == "__main__":
    main()

"""Fixed isolated transport for the pure error-budget adaptation; never parses terminal prose."""
import json
import sys

ADAPTER_SOURCE = __WORKBENCH_ADAPTER_SOURCE__


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate input key")
        result[key] = value
    return result


def invalid_constant(_value):
    raise ValueError("non-finite input")


def main():
    reply = {"task": {"id": "error-budget", "version": 1}, "status": "error",
             "runtime_version": ".".join(map(str, sys.version_info[:3])),
             "input": None, "calculation": None, "error_code": None}
    if sys.version_info < (3, 11):
        reply["error_code"] = "unsupported_runtime"
    else:
        try:
            if len(sys.argv) != 2 or len(sys.argv[1].encode("utf-8")) > 65536:
                raise ValueError("invalid input size")
            inputs = json.loads(sys.argv[1], object_pairs_hook=unique_object, parse_constant=invalid_constant)
            namespace = {"__name__": "workbench_error_budget"}
            exec(compile(ADAPTER_SOURCE, "<embedded-error-budget-adapter>", "exec"), namespace)
            try:
                result = namespace["calculate"](inputs)
            except namespace["CalculationError"] as error:
                reply["error_code"] = error.code
            else:
                reply.update(result)
                reply["status"] = "complete"
        except (ValueError, UnicodeError, RecursionError):
            reply["error_code"] = "invalid_calculation_input"
        except Exception:
            reply["error_code"] = "task_internal_error"
    # A second finite-value gate: the protocol must never contain JSON NaN or Infinity.
    sys.stdout.write(json.dumps(reply, allow_nan=False, ensure_ascii=True, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()

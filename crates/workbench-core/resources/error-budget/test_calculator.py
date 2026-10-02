"""Independent numeric assertions against the structured adapter, never parsed prose."""
import importlib.util
import math
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("calculator", Path(__file__).with_name("calculator.py"))
calculator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(calculator)


class CalculationTests(unittest.TestCase):
    def calculate(self, **inputs):
        return calculator.calculate({"slo": 99.9, **inputs})["calculation"]

    def test_time_exhaustion_clamps_positive_zero_and_preserves_units(self):
        result = self.calculate(bad_minutes=40.32)["status"]
        self.assertEqual((result["kind"], result["unit"], result["state"]), ("time", "minutes", "exhausted"))
        self.assertEqual(result["remaining"], 0)
        self.assertEqual(math.copysign(1, result["remaining"]), 1)
        self.assertAlmostEqual(result["budget"], 40.32)
        self.assertIsNone(result["observed_availability_percent"])

    def test_request_budget_has_request_units_and_observed_availability(self):
        result = self.calculate(bad_events=50, total_events=100000)["status"]
        self.assertEqual((result["kind"], result["unit"], result["state"]), ("request", "events", "ok"))
        self.assertAlmostEqual(result["budget"], 100)
        self.assertAlmostEqual(result["remaining"], 50)
        self.assertAlmostEqual(result["observed_availability_percent"], 99.95)

    def test_bound_pair_requires_both_windows_and_exposes_recovery_and_spike(self):
        expected = [(98.5, 98.5, "page", "both_windows_at_threshold"),
                    (98.5, 100, "none", "short_window_recovered"),
                    (100, 98.5, "none", "long_window_unconfirmed"),
                    (100, 100, "none", "both_below_threshold")]
        for long, short, severity, reason in expected:
            with self.subTest(long=long, short=short):
                burn = self.calculate(sli_long=long, sli_short=short)["burn"]
                self.assertEqual((burn["severity"], burn["reason"]), (severity, reason))

    def test_single_window_has_no_severity_or_projection(self):
        burn = self.calculate(sli_long=98.5)["burn"]
        self.assertEqual(burn["severity"], "not_evaluated")
        self.assertEqual(burn["reason"], "short_window_missing")
        self.assertIsNone(burn["short_rate"])
        self.assertIsNone(burn["full_budget_projection_days"])

    def test_exact_decimal_boundary_pages_but_nearby_below_threshold_does_not(self):
        exact = self.calculate(slo=99, sli_long=85.6, sli_short=85.6)["burn"]
        self.assertEqual(exact["severity"], "page")
        self.assertLess(exact["long_rate"], 14.4, "reported binary64 rate retains the original arithmetic")
        below = self.calculate(slo=99, sli_long=85.60000001, sli_short=85.60000001)["burn"]
        self.assertEqual(below["severity"], "none")
        self.assertEqual(below["reason"], "both_below_threshold")

    def test_fixed_thresholds_and_full_budget_projection_do_not_use_remaining(self):
        for horizon in (7, 28):
            burn = self.calculate(window_days=horizon, bad_minutes=1, sli_long=98.5, sli_short=98.5)["burn"]
            self.assertEqual((burn["threshold"], burn["policy_horizon_days"], burn["severity"]), (14.4, 30, "page"))
            self.assertAlmostEqual(burn["long_rate"], 15)
            self.assertAlmostEqual(burn["full_budget_projection_days"], horizon / 15)

    def test_other_bound_pairs_and_zero_burn_projection(self):
        for long, short, sli, severity in [("6h", "30m", 99, "page"), ("3d", "6h", 99.9, "ticket")]:
            burn = self.calculate(long_window=long, short_window=short, sli_long=sli, sli_short=sli)["burn"]
            self.assertEqual(burn["severity"], severity)
        burn = self.calculate(sli_long=100, sli_short=100)["burn"]
        self.assertEqual(burn["projection_state"], "zero_long_burn")
        self.assertIsNone(burn["full_budget_projection_days"])

    def test_invalid_units_pairs_ranges_and_types_are_rejected(self):
        cases = [{"slo": True}, {"slo": None}, {"slo": float("nan")}, {"slo": float("inf")}, {"slo": 100}, {"slo": 0},
                 {"window_days": 0}, {"bad_minutes": -1}, {"bad_events": 1}, {"total_events": 1},
                 {"bad_events": 2, "total_events": 1}, {"bad_events": 1, "total_events": 1, "bad_minutes": 1},
                 {"sli_short": 99}, {"sli_long": 101}, {"short_window": "30m"}, {"role": "admin"}]
        for invalid in cases:
            with self.subTest(invalid=invalid):
                with self.assertRaises(calculator.CalculationError) as raised:
                    self.calculate(**invalid)
                self.assertEqual(raised.exception.code, "invalid_calculation_input")

    def test_derived_overflow_and_positive_underflow_are_explicit_failures(self):
        for inputs in [{"window_days": 1e308, "bad_minutes": 0},
                       {"bad_minutes": 1e308}, {"total_events": 5e-324, "bad_events": 0},
                       {"window_days": 5e-324, "sli_long": 0, "sli_short": 0}]:
            with self.subTest(inputs=inputs):
                with self.assertRaises(calculator.CalculationError) as raised:
                    self.calculate(**inputs)
                self.assertEqual(raised.exception.code, "numeric_out_of_range")


if __name__ == "__main__":
    unittest.main()

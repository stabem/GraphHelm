#!/usr/bin/env python3
"""Measure paired task inference on bounded, deterministic synthetic cohorts."""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
import random
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("token_bench_doless", HERE / "doless.py")
doless = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(doless)


def synthetic_quality(rng: random.Random) -> tuple[int, int]:
    attempts = rng.randint(1, 3)
    verdicts = []
    for _ in range(attempts):
        draw = rng.random()
        verdicts.append("PASS" if draw < 0.8 else "FAIL" if draw < 0.9 else "INCOMPLETE")
    if "PASS" not in verdicts:
        verdicts[0] = "PASS"
    passed = sum(verdict == "PASS" for verdict in verdicts)
    return attempts, passed


def synthetic_arm(rng: random.Random, attempts: int, passed: int, *, cost_factor: float = 1.0) -> dict:
    expenditure = math.fsum(rng.uniform(0.8, 1.2) for _ in range(attempts)) * cost_factor
    return {
        "PASS": passed,
        "unknownCostCount": 0,
        "successFraction": passed / attempts,
        "costPerProvenDeliveryUsd": expenditure / passed,
    }


def synthetic_tasks(seed: int, task_count: int, *, effect: bool, effect_cost_factor: float) -> list[dict]:
    rng = random.Random(seed)
    tasks = []
    for index in range(task_count):
        attempts, passed = synthetic_quality(rng)
        baseline = synthetic_arm(rng, attempts, passed)
        candidate = synthetic_arm(rng, attempts, passed,
                                 cost_factor=effect_cost_factor if effect else 1.0)
        tasks.append({
            "task": f"synthetic-{index + 1}",
            "baseline": baseline,
            "candidate": candidate,
            "deltaCandidateMinusBaseline": {
                "successFraction": candidate["successFraction"] - baseline["successFraction"],
                "costPerProvenDeliveryUsd": (
                    candidate["costPerProvenDeliveryUsd"] - baseline["costPerProvenDeliveryUsd"]
                ),
            },
        })
    return tasks


def calibrate(seed: int, cohorts: int, task_counts: list[int], minimum_cost_reduction_usd: float,
              maximum_quality_regression: float, effect_cost_factor: float) -> dict:
    scenarios = {"null": [], "effect": []}
    for task_count in task_counts:
        for scenario, effect in (("null", False), ("effect", True)):
            seeds = [seed + index for index in range(cohorts)]
            counts = {name: 0 for name in
                      ("supported", "inconclusive", "quality_regression", "not_computable")}
            for cohort_seed in seeds:
                tasks = synthetic_tasks(cohort_seed, task_count, effect=effect,
                                        effect_cost_factor=effect_cost_factor)
                verdict = doless.paired_task_inference(
                    tasks, minimum_cost_reduction_usd, maximum_quality_regression
                )["inferenceStatus"]
                counts[verdict] += 1
            scenarios[scenario].append({
                "taskCount": task_count,
                "seeds": seeds,
                "verdictCounts": counts,
            })
    return {
        "method": "existing_pair_summary_paired_task_decision",
        "seed": seed,
        "cohorts": cohorts,
        "taskCounts": task_counts,
        "thresholds": {
            "minimumCostReductionUsd": minimum_cost_reduction_usd,
            "maximumQualityRegression": maximum_quality_regression,
            "effectCostFactor": effect_cost_factor,
        },
        "scenarios": scenarios,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=183)
    parser.add_argument("--cohorts", type=int, default=40)
    parser.add_argument("--task-counts", type=int, nargs="+", default=[6])
    parser.add_argument("--minimum-cost-reduction-usd", type=float, default=0.5)
    parser.add_argument("--maximum-quality-regression", type=float, default=0.2)
    parser.add_argument("--effect-cost-factor", type=float, default=0.2)
    args = parser.parse_args(argv)
    if not 1 <= args.cohorts <= 200:
        print("error: cohorts must be between 1 and 200", file=sys.stderr)
        return 1
    if len(args.task_counts) > 12:
        print("error: task counts must contain at most 12 entries", file=sys.stderr)
        return 1
    if len(set(args.task_counts)) != len(args.task_counts):
        print("error: task counts must be unique", file=sys.stderr)
        return 1
    if any(not 1 <= count <= 12 for count in args.task_counts):
        print("error: each task count must be between 1 and 12", file=sys.stderr)
        return 1
    if (not math.isfinite(args.minimum_cost_reduction_usd) or args.minimum_cost_reduction_usd <= 0 or
            not math.isfinite(args.maximum_quality_regression) or
            not 0 <= args.maximum_quality_regression <= 1 or
            not math.isfinite(args.effect_cost_factor) or not 0 < args.effect_cost_factor < 1):
        print("error: thresholds must be finite and effect-cost-factor must be between 0 and 1", file=sys.stderr)
        return 1
    report = calibrate(args.seed, args.cohorts, args.task_counts,
                       args.minimum_cost_reduction_usd, args.maximum_quality_regression,
                       args.effect_cost_factor)
    print(json.dumps(report, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Binary-size budget gate (G-19), run by .github/workflows/budgets.yml.

Measures one built binary, writes a JSON record, and exits non-zero when
  - the binary is over the hard limit in benches/budgets.json, or
  - it grew more than `max_regression_pct` over the stored baseline for this target.

A target with no baseline (null) is only checked against the hard limit; its record
is what you copy into budgets.json to start tracking it.

Usage:
  python3 scripts/check_budgets.py --target aarch64-apple-darwin \
      --binary target/aarch64-apple-darwin/release/api-tester \
      [--budgets benches/budgets.json] [--out budgets-result.json]

Standard library only, so it runs on any runner without a pip install.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
from pathlib import Path

MIB = 1024 * 1024


def evaluate(size: int, target: str, budgets: dict) -> dict:
    """Pure check: returns the record, with `ok` and the reasons it failed."""
    section = budgets["binary_size"]
    limit = int(section["limit_bytes"])
    max_pct = float(budgets["max_regression_pct"])
    baseline_entry = section.get("baselines", {}).get(target)
    baseline = int(baseline_entry["bytes"]) if baseline_entry else None

    failures = []
    if size > limit:
        failures.append(f"size {size / MIB:.2f} MB is over the {limit / MIB:.0f} MB limit")

    delta_pct = None
    if baseline:
        delta_pct = (size - baseline) / baseline * 100.0
        if delta_pct > max_pct:
            failures.append(
                f"size grew {delta_pct:+.1f}% over the baseline "
                f"({baseline / MIB:.2f} MB), more than the allowed {max_pct:.0f}%"
            )

    return {
        "metric": "binary_size",
        "target": target,
        "bytes": size,
        "mb": round(size / MIB, 2),
        "limit_bytes": limit,
        "baseline_bytes": baseline,
        "delta_pct": None if delta_pct is None else round(delta_pct, 2),
        "max_regression_pct": max_pct,
        "ok": not failures,
        "failures": failures,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", required=True, help="Rust target triple the binary was built for")
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--budgets", default=Path("benches/budgets.json"), type=Path)
    parser.add_argument("--out", type=Path, help="Where to write the JSON record (default: stdout only)")
    args = parser.parse_args(argv)

    budgets = json.loads(args.budgets.read_text(encoding="utf-8"))
    record = evaluate(args.binary.stat().st_size, args.target, budgets)
    record["binary"] = str(args.binary)
    record["commit"] = os.environ.get("GITHUB_SHA")
    record["measured_at"] = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")

    text = json.dumps(record, indent=2)
    print(text)
    if args.out:
        args.out.write_text(text + "\n", encoding="utf-8")

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        baseline = record["baseline_bytes"]
        delta = record["delta_pct"]
        with open(summary, "a", encoding="utf-8") as fh:
            fh.write(
                f"### Binary size: `{record['target']}`\n\n"
                f"| Size | Limit | Baseline | Change | Verdict |\n|---|---|---|---|---|\n"
                f"| {record['mb']} MB ({record['bytes']:,} B) "
                f"| {record['limit_bytes'] / MIB:.0f} MB "
                f"| {'none' if baseline is None else f'{baseline / MIB:.2f} MB'} "
                f"| {'n/a' if delta is None else f'{delta:+.1f}%'} "
                f"| {'pass' if record['ok'] else 'FAIL'} |\n\n"
            )

    for failure in record["failures"]:
        # GitHub annotation when on Actions, plain text otherwise.
        print(f"::error::{record['target']}: {failure}", file=sys.stderr)
    return 0 if record["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())

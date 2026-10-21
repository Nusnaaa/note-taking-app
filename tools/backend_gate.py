#!/usr/bin/env python3
"""Report the backend gate; incomplete, stale or missing evidence fails closed."""
import csv
import hashlib
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
reasons = []
try:
    gaps = json.loads((root / "docs/verification/backend-gaps.json").read_text())
    reasons += [g["id"] for g in gaps if g["status"] != "CLOSED"]
    evidence = json.loads((root / "docs/evidence/latest/validation.json").read_text())
    required_checks = {"format", "lint", "tests", "release-build", "traceability"}
    observed = {c["name"]: c for c in evidence["checks"]}
    if any(name not in observed or observed[name]["result"] != "PASS" or observed[name]["exit_code"] != 0 for name in required_checks):
        reasons.append("AUTOMATED_CHECKS_INCOMPLETE")
    if not evidence.get("finished_utc") or not evidence.get("totals", {}).get("passed") or evidence.get("working_tree_before"):
        reasons.append("INCOMPLETE_OR_UNCOMMITTED_RUN")
    if evidence.get("totals", {}).get("failed") or evidence.get("totals", {}).get("ignored"):
        reasons.append("FAILED_OR_IGNORED_TESTS")
    for path, expected in evidence["source_sha256"].items():
        if hashlib.sha256((root / path).read_bytes()).hexdigest() != expected:
            reasons.append("STALE_SOURCE:" + path)
    if not evidence["source_sha256"]:
        reasons.append("MISSING_SOURCE_MANIFEST")
    tracked = subprocess.check_output(["git", "ls-files"], cwd=root, text=True).splitlines()
    new_sources = [p for p in tracked if not p.startswith("docs/evidence/") and p not in evidence["source_sha256"]]
    reasons += ["UNTESTED_INPUT:" + p for p in new_sources]
    requirements = list(csv.DictReader((root / "docs/verification/requirements-current.csv").open()))
    if len(requirements) != 94 or any(r["initial_release"] == "YES" and r["backend_state"] not in ("COMPLETE", "POST_HI") for r in requirements):
        reasons.append("REQUIREMENT_COVERAGE_INCOMPLETE")
except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
    reasons.append("MISSING_OR_INVALID_EVIDENCE:" + type(error).__name__)

accepted = not reasons
print(json.dumps({"backend_accepted": accepted, "production_hi_allowed": accepted, "reasons": reasons}, indent=2))
raise SystemExit(0 if accepted else 2)

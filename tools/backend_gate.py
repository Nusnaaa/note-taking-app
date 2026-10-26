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
    platform = json.loads((root / "docs/evidence/latest/android-validation.json").read_text())
    platform_checks = {c["name"]: c for c in platform["checks"]}
    if any(platform_checks.get(name, {}).get("result") != "PASS" for name in ("android-native", "android-elf", "jni-library", "jni-compile", "jni-smoke", "jni-shrinker-version", "jni-shrink", "jni-shrunk-smoke", "android-gradle")):
        reasons.append("ANDROID_AUTOMATED_CHECKS_INCOMPLETE")
    if platform.get("working_tree_before") or not platform.get("finished_utc") or platform.get("merged_manifest_policy") != "PASS":
        reasons.append("ANDROID_RUN_INCOMPLETE_OR_UNCOMMITTED")
    if platform.get("source_sha256") != evidence["source_sha256"]:
        reasons.append("ANDROID_SOURCE_REVISION_MISMATCH")
    if platform.get("jni_shrink_policy") != "PASS" or platform.get("consumer_rules_policy") != "PASS" or platform.get("jni_smoke_groups", 0) < 40 or platform.get("jni_shrunk_smoke_groups") != platform.get("jni_smoke_groups") or not platform.get("release_aar_sha256"):
        reasons.append("JNI_RELEASE_PACKAGING_INCOMPLETE")
    totals = platform.get("android_tests", {})
    if totals.get("tests", 0) < 10 or any(totals.get(k, 0) for k in ("failures", "errors", "skipped")) or not platform.get("packaged_arm64_library_bytes"):
        reasons.append("ANDROID_TEST_RESULTS_INCOMPLETE")
    requirements = list(csv.DictReader((root / "docs/verification/requirements-current.csv").open()))
    if len(requirements) != 94 or any(r["initial_release"] == "YES" and r["backend_state"] not in ("COMPLETE", "POST_HI") for r in requirements):
        reasons.append("REQUIREMENT_COVERAGE_INCOMPLETE")
    initial = {r["requirement_id"] for r in requirements if r["initial_release"] == "YES" and r["backend_state"] != "POST_HI"}
    inherited = list(csv.DictReader((root / "docs/verification/inherited-current.csv").open()))
    if len(inherited) != 103 or any(r["current_semantic_requirement"] in initial and r["semantic_closure"] != "PASS" for r in inherited):
        reasons.append("INHERITED_CLAUSE_COVERAGE_INCOMPLETE")
except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
    reasons.append("MISSING_OR_INVALID_EVIDENCE:" + type(error).__name__)

accepted = not reasons
print(json.dumps({"backend_accepted": accepted, "production_hi_allowed": accepted, "reasons": reasons}, indent=2))
raise SystemExit(0 if accepted else 2)

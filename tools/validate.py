#!/usr/bin/env python3
"""Execute real checks and retain output against a precise Git revision.

Cargo --offline constrains dependency access. It is not a claim that an arbitrary
future vendor SDK has passed a runtime egress audit.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default="docs/evidence/latest")
    args = parser.parse_args()
    destination = (ROOT / args.output).resolve()
    destination.mkdir(parents=True, exist_ok=True)
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
    commands = [
        ("format", [cargo, "fmt", "--all", "--", "--check"]),
        ("lint", [cargo, "clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"]),
        ("tests", [cargo, "test", "--workspace", "--locked", "--offline"]),
        ("release-build", [cargo, "build", "--workspace", "--release", "--locked", "--offline"]),
        ("traceability", [sys.executable, "tools/traceability.py", "--check"]),
    ]
    git = lambda *a: subprocess.check_output(["git", *a], cwd=ROOT, text=True).strip()
    revision = git("rev-parse", "HEAD")
    full_status = subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).rstrip()
    source_status = "\n".join(line for line in full_status.splitlines() if not line[3:].startswith("docs/evidence/"))
    tracked = git("ls-files").splitlines()
    # Evidence is excluded from its own fingerprint; all implementation/test inputs remain.
    hashes = {p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in tracked if not p.startswith("docs/evidence/")}
    os.environ["CARGO_INCREMENTAL"] = "0"
    os.environ["CARGO_BUILD_JOBS"] = "2"
    report = {
        "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_commit": revision,
        "working_tree_before": source_status,
        "full_working_tree_before": full_status,
        "build_environment": {"CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": "2"},
        "platform": os.uname().sysname + " " + os.uname().machine,
        "cargo": subprocess.check_output([cargo, "--version"], text=True).strip(),
        "source_sha256": hashes,
        "checks": [],
        "tests": [],
        "backend_accepted": False,
    }
    for name, command in commands:
        start = time.monotonic()
        result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
        # Successful silent commands still need a useful, nonempty evidence file.
        # Preserve captured output verbatim beneath explicit invocation metadata.
        log = "$ " + shlex.join(command) + "\nexit_code=" + str(result.returncode) + "\n\n" + result.stdout
        (destination / f"{name}.log").write_text(log)
        entry = {"name": name, "command": command, "exit_code": result.returncode, "result": "PASS" if result.returncode == 0 else "FAIL", "seconds": round(time.monotonic() - start, 3), "log": f"{name}.log"}
        report["checks"].append(entry)
        if name == "tests":
            report["tests"] = [{"name": n, "result": s} for n, s in re.findall(r"^test ([\w:]+) \.\.\. (ok|FAILED|ignored)$", result.stdout, re.MULTILINE)]
        (destination / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
        print(f"{name}: {entry['result']} (exit {result.returncode})", flush=True)
        if result.returncode:
            print(result.stdout[-6000:], flush=True)
            return result.returncode
    report["finished_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["totals"] = {"passed": sum(t["result"] == "ok" for t in report["tests"]), "failed": sum(t["result"] == "FAILED" for t in report["tests"]), "ignored": sum(t["result"] == "ignored" for t in report["tests"])}
    (destination / "validation.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["totals"]), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

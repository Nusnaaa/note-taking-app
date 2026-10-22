#!/usr/bin/env python3
"""Record pinned dependency declarations and bundled licence-file evidence."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
cargo = shutil.which("cargo") or str(Path.home() / ".cargo/bin/cargo")
metadata = json.loads(subprocess.check_output([cargo, "metadata", "--locked", "--offline", "--format-version", "1"], cwd=root))
packages = []
for package in metadata["packages"]:
    if package["source"] is None:
        continue
    directory = Path(package["manifest_path"]).parent
    licences = [p for p in directory.iterdir() if p.is_file() and ("license" in p.name.lower() or "copying" in p.name.lower())]
    declared = package["license"] or ""
    if "Unicode-3.0" in declared and "MIT" in declared:
        chosen = "MIT AND Unicode-3.0"
    elif "MIT" in declared:
        chosen = "MIT"
    elif "BSD-3-Clause" in declared:
        chosen = "BSD-3-Clause"
    elif "BSD-2-Clause" in declared:
        chosen = "BSD-2-Clause"
    elif declared == "Zlib":
        chosen = "Zlib"
    elif "Apache-2.0" in declared:
        chosen = "Apache-2.0"
    else:
        raise ValueError(f"Unreviewed licence expression for {package['name']}: {declared}")
    entry = {"name": package["name"], "version": package["version"], "declared_spdx": package["license"], "selected_permitted_route": chosen, "repository": package["repository"], "registry_source": package["source"], "licence_files_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(licences)}, "binary_redistribution": "Target-specific notices and bundled components must be assembled/reviewed before a binary release; no binary distributed here."}
    if not licences:
        entry["review_note"] = "Declaration found in Cargo metadata/README; no top-level licence text in fetched crate. Preserve as a release-review item, not a completed notice audit."
    if package["name"] == "libsqlite3-sys":
        sqlite = directory / "sqlite3/sqlite3.c"
        entry["bundled_component"] = {"name": "SQLite", "version": "3.46.0", "licence": "SQLite source copyright disclaimer in sqlite3.c; wrapper is MIT", "sha256": hashlib.sha256(sqlite.read_bytes()).hexdigest(), "sqlcipher": "Not enabled"}
    packages.append(entry)
destination = root / "docs/licensing/dependency-inventory.json"
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text(json.dumps({"scope": "All resolved Cargo targets, including build/dev and inactive platform dependencies; not a shipped-binary component claim", "packages": packages}, indent=2) + "\n")
print(f"Recorded {len(packages)} pinned registry packages and available licence-file hashes.")

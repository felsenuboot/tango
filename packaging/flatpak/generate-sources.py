#!/usr/bin/env python3
"""Generate offline Flatpak sources from Cargo.lock (Python 3.11+, no downloads).

Only crates.io dependencies are supported; fail rather than silently omitting
git dependencies. Regenerate after changing Cargo.lock; --check is for CI.
"""

import argparse
import json
from pathlib import Path
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    sources = []
    for package in lock["package"]:
        if "source" not in package:
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise SystemExit(f"Unsupported source for {package['name']}: {package['source']}")
        name, version, checksum = (package[key] for key in ("name", "version", "checksum"))
        dest = f"cargo/vendor/{name}-{version}"
        sources.extend([
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                "sha256": checksum,
                "dest": dest,
            },
            {
                "type": "inline",
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": dest,
                "dest-filename": ".cargo-checksum.json",
            },
        ])
    sources.append({
        "type": "inline",
        "contents": '[source.crates-io]\nreplace-with = "vendored-sources"\n'
                    '[source.vendored-sources]\ndirectory = "cargo/vendor"\n',
        "dest": "cargo",
        "dest-filename": "config.toml",
    })
    output = Path(__file__).with_name("cargo-sources.json")
    contents = json.dumps(sources, indent=2) + "\n"
    if args.check:
        if not output.exists() or output.read_text() != contents:
            raise SystemExit("Run python3 packaging/flatpak/generate-sources.py and commit the result.")
    else:
        output.write_text(contents)


if __name__ == "__main__":
    main()

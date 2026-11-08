"""Write THIRD-PARTY-NOTICES.txt for the release zip: every crate compiled into the mod's library on Windows, Linux
or macOS (from `cargo metadata`), with its license files. Usage: python scripts/notices.py <out file>
"""

import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]


def crates():
    args = ["cargo", "metadata", "--format-version", "1"]
    for t in TARGETS:
        args += ["--filter-platform", t]
    meta = json.loads(subprocess.check_output(args, cwd=ROOT))
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]

    def proc_macro(i):  # runs at build time only, nothing of it ships
        return any("proc-macro" in t["kind"] for t in pkgs[i]["targets"])

    seen, stack = set(), [root]
    while stack:  # normal (runtime) dependencies only
        for dep in nodes[stack.pop()]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in seen and not proc_macro(dep["pkg"]):
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    return pkgs[root]["name"], sorted((pkgs[i] for i in seen), key=lambda p: p["name"])


def license_files(pkg):
    d = pathlib.Path(pkg["manifest_path"]).parent
    return sorted(f for f in d.iterdir() if f.is_file() and re.match(r"(LICEN[CS]E|COPYING|NOTICE)", f.name, re.I))


def main(out):
    rule = "=" * 78
    name, deps = crates()
    parts = [f"{name} includes the following third-party software, compiled into its library.\n"]
    for p in deps:
        head = f"{p['name']} {p['version']}\n{p.get('repository') or 'https://crates.io/crates/' + p['name']}\n{p['license']}"
        files = license_files(p)
        body = "\n\n".join(f"--- {f.name} ---\n{f.read_text(encoding='utf-8', errors='replace').strip()}" for f in files)
        parts += [rule, head, rule, body or f"No license file ships with this crate. It is licensed under {p['license']}; see its repository.", ""]
    pathlib.Path(out).write_text("\n".join(parts), encoding="utf-8")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "THIRD-PARTY-NOTICES.txt")

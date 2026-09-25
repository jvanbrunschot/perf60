#!/usr/bin/env python3
"""Resolve merge conflicts in the shared registries when integrating parallel feature branches.

Usage: scripts/resolve-registry.py src/checks/mod.rs [src/procfs/mod.rs ...]

Both files only ever conflict because two branches each added lines. The resolver rebuilds them
from the union of both sides instead of doing a line-level merge:
  - `pub mod` lines: unique and sorted
  - registry entries (`Box::new(module::Type::default())`): unique, in checklist order (ORDER)
Everything else in the file must be identical on both sides; the resolver keeps it once.
"""
import re
import sys

# Section order in the report: Gregg's 60-second checklist, then the newer checks.
# Keys are the snake_case check type (CpuBalance -> cpu_balance) or the module name.
ORDER = [
    "load", "kernel_log", "cpu", "cpu_balance", "processes", "disk", "memory", "swap",
    "net_dev", "net", "tcp", "pressure",
    "cgroup", "cgroups_top", "sockets", "filesystems", "limits", "hardware",
    # --deep (eBPF) sections
    "runqlat", "biolatency", "execsnoop", "tcpretrans",
]
MARK = re.compile(r"^(<<<<<<<|=======|>>>>>>>|\|\|\|\|\|\|\|)")


def snake(name):
    return re.sub(r"(?<!^)([A-Z])", r"_\1", name).lower()


def entry_key(line):
    m = re.search(r"Box::new\((\w+)::(\w+)::default", line)
    if not m:
        return len(ORDER)
    for name in (snake(m.group(2)), m.group(1)):
        if name in ORDER:
            return ORDER.index(name)
    return len(ORDER)


def uniq(lines):
    seen, out = set(), []
    for line in lines:
        key = re.sub(r"\s+", " ", line.strip())
        if key not in seen:
            seen.add(key)
            out.append(line)
    return out


def rebuild(path):
    lines = [l for l in open(path).read().split("\n") if not MARK.match(l)]
    is_mod = lambda l: re.match(r"^\s*pub mod \w+;\s*,?$", l) is not None
    mods = sorted(uniq([l.strip().rstrip(",") for l in lines if is_mod(l)]))
    first = next(i for i, l in enumerate(lines) if is_mod(l))
    head = lines[:first]
    if path.endswith("checks/mod.rs"):
        entries = sorted(uniq([l for l in lines if "Box::new(" in l]), key=entry_key)
        body = ["", "pub fn all() -> Vec<Box<dyn Check>> {", "    vec!["] + entries + ["    ]", "}", ""]
        out = head + mods + body
    else:
        last = max(i for i, l in enumerate(lines) if is_mod(l))
        tail = [l for l in lines[last + 1:] if not is_mod(l)]
        out = head + mods + tail
    with open(path, "w") as f:
        f.write("\n".join(out))
    print("rebuilt", path)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    for p in sys.argv[1:]:
        rebuild(p)

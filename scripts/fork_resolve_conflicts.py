#!/usr/bin/env python3
"""Resolve upstream-merge conflicts where the fork only added lines.

Run during an in-progress merge, after scripts/fork_merge_upstream.sh has
taken upstream's side for paths the fork never changed. For every remaining
conflicted path, each conflict block is resolved when the fork's side is the
merge base plus inserted lines: upstream's side is kept and the fork's lines
are inserted next to the same neighbouring lines. A block where the fork
changed or removed base lines, an ambiguous anchor, or a path missing a
merge stage is left unresolved.

Generated files are taken from upstream; the merge script regenerates them.

Prints the paths it could not resolve and exits 1 when there are any.
"""

from __future__ import annotations

import difflib
import subprocess
import sys
import tempfile
from pathlib import Path

GENERATED = ("docs/next/api/herdr-api.schema.json",)
MARKER = 7


def git(*args: str, check: bool = True) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(["git", *args], check=check, capture_output=True)


def stage(number: int, path: str) -> bytes | None:
    result = git("show", f":{number}:{path}", check=False)
    return result.stdout if result.returncode == 0 else None


def conflicted_paths() -> list[str]:
    output = git("diff", "--name-only", "--diff-filter=U", "-z").stdout
    return [path for path in output.decode().split("\0") if path]


def find_unique(lines: list[str], wanted: str) -> int | None:
    if not wanted.strip():
        return None
    matches = [index for index, line in enumerate(lines) if line == wanted]
    return matches[0] if len(matches) == 1 else None


def resolve_block(base: list[str], ours: list[str], theirs: list[str]) -> list[str] | None:
    """Upstream's lines plus the fork's inserted lines, or None."""
    inserts: dict[int, list[str]] = {}
    matcher = difflib.SequenceMatcher(a=base, b=ours, autojunk=False)
    for tag, base_start, _, ours_start, ours_end in matcher.get_opcodes():
        if tag == "equal":
            continue
        if tag != "insert":
            return None
        inserts.setdefault(base_start, []).extend(ours[ours_start:ours_end])
    result = list(theirs)
    for index in sorted(inserts, reverse=True):
        if index == len(base):
            position = len(result)
        elif index == 0:
            position = 0
        else:
            before = find_unique(result, base[index - 1])
            after = find_unique(result, base[index])
            if before is not None:
                position = before + 1
            elif after is not None:
                position = after
            else:
                return None
        result[position:position] = inserts[index]
    return result


def resolve_text(merged: list[str]) -> list[str] | None:
    """Resolves every diff3 conflict block in `merged`, or returns None."""
    output: list[str] = []
    index = 0
    while index < len(merged):
        line = merged[index]
        if not line.startswith("<" * MARKER):
            output.append(line)
            index += 1
            continue
        sections: dict[str, list[str]] = {"ours": [], "base": [], "theirs": []}
        current = "ours"
        index += 1
        while index < len(merged) and not merged[index].startswith(">" * MARKER):
            text = merged[index]
            if text.startswith("|" * MARKER):
                current = "base"
            elif text.startswith("=" * MARKER) and current == "base":
                current = "theirs"
            else:
                sections[current].append(text)
            index += 1
        if index >= len(merged):
            return None
        index += 1
        block = resolve_block(sections["base"], sections["ours"], sections["theirs"])
        if block is None:
            return None
        output.extend(block)
    return output


def resolve_path(path: str) -> bool:
    if path in GENERATED:
        git("checkout", "--theirs", "--", path)
        git("add", "--", path)
        return True
    base, ours, theirs = (stage(number, path) for number in (1, 2, 3))
    if base is None or ours is None or theirs is None:
        return False
    with tempfile.TemporaryDirectory() as directory:
        files = []
        for name, content in (("ours", ours), ("base", base), ("theirs", theirs)):
            file = Path(directory) / name
            file.write_bytes(content)
            files.append(str(file))
        merged = git(
            "merge-file", "-p", "--diff3", "-L", "ours", "-L", "base", "-L", "theirs",
            *files, check=False,
        )
    if merged.returncode < 0:
        return False
    try:
        text = merged.stdout.decode()
    except UnicodeDecodeError:
        return False
    resolved = resolve_text(text.splitlines(keepends=True))
    if resolved is None:
        return False
    Path(path).write_text("".join(resolved))
    git("add", "--", path)
    return True


def main() -> int:
    unresolved = [path for path in conflicted_paths() if not resolve_path(path)]
    for path in conflicted_paths():
        if path not in unresolved:
            unresolved.append(path)
    if unresolved:
        print("Conflicts the fork did not only add lines to:", file=sys.stderr)
        for path in unresolved:
            print(f"  {path}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Tests for scripts/fork_resolve_conflicts.py."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fork_resolve_conflicts as resolver

SCRIPT = Path(__file__).resolve().parent / "fork_resolve_conflicts.py"


def lines(*items: str) -> list[str]:
    return [f"{item}\n" for item in items]


class ResolveBlockTests(unittest.TestCase):
    def test_fork_insertions_follow_their_neighbours_in_upstreams_version(self) -> None:
        base = lines("mod a;", "mod c;")
        ours = lines("mod a;", "mod b;", "mod c;")
        theirs = lines("mod a;", "mod c2;")
        self.assertEqual(
            resolver.resolve_block(base, ours, theirs),
            lines("mod a;", "mod b;", "mod c2;"),
        )

    def test_insertions_at_the_edges_stay_at_the_edges(self) -> None:
        base = lines("x", "y")
        ours = lines("first", "x", "y", "last")
        theirs = lines("x2", "y2")
        self.assertEqual(
            resolver.resolve_block(base, ours, theirs),
            lines("first", "x2", "y2", "last"),
        )

    def test_both_sides_adding_at_the_same_spot_keeps_both(self) -> None:
        self.assertEqual(
            resolver.resolve_block([], lines("fork"), lines("upstream")),
            lines("upstream", "fork"),
        )

    def test_a_fork_that_changed_base_lines_is_not_resolved(self) -> None:
        base = lines("a", "b")
        ours = lines("a", "b changed")
        theirs = lines("a", "b upstream")
        self.assertIsNone(resolver.resolve_block(base, ours, theirs))

    def test_a_missing_or_ambiguous_anchor_is_not_resolved(self) -> None:
        base = lines("a", "c", "d")
        ours = lines("a", "c", "b", "d")
        self.assertIsNone(resolver.resolve_block(base, ours, lines("x", "y")))
        self.assertIsNone(resolver.resolve_block(base, ours, lines("c", "c")))


class ResolvePathTests(unittest.TestCase):
    def git(self, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *args], cwd=self.repo, check=check, capture_output=True, text=True
        )

    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        self.git("init", "-q", "-b", "upstream")
        self.git("config", "user.name", "test")
        self.git("config", "user.email", "test@example.com")

    def tearDown(self) -> None:
        self.temp.cleanup()

    def commit(self, path: str, text: str, message: str) -> None:
        (self.repo / path).write_text(text)
        self.git("add", path)
        self.git("commit", "-qm", message)

    def run_resolver(self) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT)], cwd=self.repo, capture_output=True, text=True
        )

    def test_merges_conflicts_where_the_fork_only_added_lines(self) -> None:
        self.commit("list.rs", "mod a;\nmod c;\n", "base")
        self.git("checkout", "-qb", "fork")
        self.commit("list.rs", "mod a;\nmod b;\nmod c;\n", "fork")
        self.git("checkout", "-q", "upstream")
        self.commit("list.rs", "mod a;\nmod c2;\n", "upstream")
        self.git("checkout", "-q", "fork")
        self.assertNotEqual(self.git("merge", "--no-commit", "upstream", check=False).returncode, 0)
        result = self.run_resolver()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.repo / "list.rs").read_text(), "mod a;\nmod b;\nmod c2;\n")
        self.assertEqual(self.git("diff", "--name-only", "--diff-filter=U").stdout, "")

    def test_leaves_real_conflicts_for_a_human(self) -> None:
        self.commit("code.rs", "let x = 1;\n", "base")
        self.git("checkout", "-qb", "fork")
        self.commit("code.rs", "let x = 2;\n", "fork")
        self.git("checkout", "-q", "upstream")
        self.commit("code.rs", "let x = 3;\n", "upstream")
        self.git("checkout", "-q", "fork")
        self.git("merge", "--no-commit", "upstream", check=False)
        result = self.run_resolver()
        self.assertEqual(result.returncode, 1)
        self.assertIn("code.rs", result.stderr)
        self.assertIn("<<<<<<<", (self.repo / "code.rs").read_text())


if __name__ == "__main__":
    unittest.main()

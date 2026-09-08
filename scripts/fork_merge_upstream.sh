#!/usr/bin/env bash
# Merge an upstream herdr release tag into the fork branch.
#
# Upstream cuts each stable tag on a side branch, so the previous base tag is
# never an ancestor of the next one and a plain merge conflicts on release
# files (Cargo.toml, CHANGELOG.md, docs/next, ...) even where the fork changed
# nothing. Conflicted paths whose fork side still matches the previous upstream
# base are taken from upstream. In paths the fork changed, conflicts where the
# fork only added lines are resolved by scripts/fork_resolve_conflicts.py.
# Anything else is left for manual resolution, with the merge still in
# progress. When cargo is available, the generated API schema is regenerated
# before committing.
#
# Usage: scripts/fork_merge_upstream.sh vX.Y.Z
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 vX.Y.Z" >&2
  exit 2
fi
TAG="$1"
PREV_TAG="v$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
if ! git rev-parse -q --verify "refs/tags/${PREV_TAG}" >/dev/null; then
  echo "error: previous upstream base tag ${PREV_TAG} is not available locally" >&2
  exit 1
fi

SCHEMA=docs/next/api/herdr-api.schema.json

merge_status=0
git merge --no-commit --no-ff --no-edit "$TAG" || merge_status=$?
if ! git rev-parse -q --verify MERGE_HEAD >/dev/null; then
  if [ "$merge_status" -ne 0 ]; then
    echo "error: merging ${TAG} failed before any conflict resolution" >&2
    exit 1
  fi
  echo "Already up to date with ${TAG}."
  exit 0
fi

git diff --name-only --diff-filter=U -z | while IFS= read -r -d '' path; do
  # Compare blobs rather than paths so a fork change is never dropped when
  # upstream renames the file it lives in.
  ours=$(git rev-parse -q --verify ":2:${path}" || true)
  base=$(git rev-parse -q --verify "${PREV_TAG}:${path}" || true)
  [ "$ours" = "$base" ] || continue
  if git rev-parse -q --verify ":3:${path}" >/dev/null; then
    git checkout --theirs -- "$path"
    git add -- "$path"
  else
    git rm -q -- "$path"
  fi
  echo "took upstream (unchanged in fork since ${PREV_TAG}): ${path}"
done

if [ -n "$(git diff --name-only --diff-filter=U)" ]; then
  python3 scripts/fork_resolve_conflicts.py || true
fi

remaining=$(git diff --name-only --diff-filter=U)
if [ -n "$remaining" ]; then
  echo "Merge of ${TAG} has conflicts in paths the fork changed:" >&2
  printf '  %s\n' $remaining >&2
  exit 1
fi

if command -v cargo >/dev/null; then
  HERDR_UPDATE_API_SCHEMA=1 cargo test --locked --bin herdr generated_protocol_schema_artifact_is_current -q
  git add -- "$SCHEMA"
else
  echo "cargo not found; not regenerating ${SCHEMA}" >&2
fi
git commit --no-edit

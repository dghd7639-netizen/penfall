#!/usr/bin/env bash
# Builds the public copy of this repository from its main branch: the same
# history, with docs/personal-mvp.md removed from every commit and the owner's
# commit email replaced by the GitHub noreply address. The output is
# deterministic, so re-running it after new commits only adds commits on top
# and the public repository can be updated with a plain push.
#
#   scripts/export-public.sh <empty-output-dir>
#   git -C <output-dir> push <public-remote> main
set -euo pipefail

out=${1:?usage: scripts/export-public.sh <empty-output-dir>}
src=$(cd "$(dirname "$0")/.." && pwd)
# The private address is read from git config so it is never written here.
private_email=${PRIVATE_EMAIL:-$(git -C "$src" config user.email)}
public_email="270913790+dghd7639-netizen@users.noreply.github.com"
if [ -z "$private_email" ]; then
  echo "error: set PRIVATE_EMAIL or git config user.email" >&2
  exit 1
fi

if [ -e "$out" ] && [ -n "$(ls -A "$out")" ]; then
  echo "error: $out is not empty" >&2
  exit 1
fi

git clone --quiet --no-local --single-branch --branch main "$src" "$out"
cd "$out"
git remote remove origin

FILTER_BRANCH_SQUELCH_WARNING=1 git filter-branch --force --prune-empty \
  --index-filter 'git rm --cached --ignore-unmatch --quiet docs/personal-mvp.md' \
  --env-filter "
    if [ \"\$GIT_AUTHOR_EMAIL\" = '$private_email' ]; then GIT_AUTHOR_EMAIL='$public_email'; fi
    if [ \"\$GIT_COMMITTER_EMAIL\" = '$private_email' ]; then GIT_COMMITTER_EMAIL='$public_email'; fi
  " -- main

# Drop the pre-rewrite objects so nothing private remains in this copy.
git for-each-ref --format='%(refname)' refs/original | xargs -r -n1 git update-ref -d
git reflog expire --expire=now --all
git gc --quiet --prune=now

echo "public copy ready: $out ($(git rev-list --count main) commits, main at $(git rev-parse --short main))"

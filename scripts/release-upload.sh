#!/bin/bash
# Uploads files to a release and proves GitHub holds their exact bytes: four tries per file
# (GitHub's asset API fails often enough to cost a release; `--clobber` replaces a half-saved
# asset), then GitHub's SHA-256 of the asset must equal the local file's. The release pipeline's
# Windows publish job runs it (`release-pipeline.yml`, `windows-publish`); the macOS `sign` job
# carries the same loop inline, from before this script.
#
# Usage: ./scripts/release-upload.sh <tag> <release id> <file>...
# Needs GH_TOKEN and GITHUB_REPOSITORY.
set -euo pipefail

TAG="$1"
RELEASE_ID="$2"
shift 2

upload() {
  local file="$1" name attempt want got
  name=$(basename "$file")
  for attempt in 1 2 3 4; do
    if gh release upload "$TAG" -R "$GITHUB_REPOSITORY" "$file" --clobber; then
      break
    fi
    if [ "$attempt" = 4 ]; then
      echo "::error::Couldn't upload $name after four tries."
      return 1
    fi
    sleep $((attempt * 15))
  done
  want="sha256:$(shasum -a 256 "$file" | cut -d' ' -f1)"
  # A fresh asset may not show its digest yet, so an empty one gets two more looks.
  for attempt in 1 2 3; do
    got=$(gh api "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" \
      --jq ".assets[] | select(.name == \"$name\") | .digest // empty") || got=""
    [ -n "$got" ] && break
    sleep 5
  done
  if [ "$got" != "$want" ]; then
    echo "::error::$name on the release has digest '$got', but this job built $want."
    return 1
  fi
  echo "Uploaded $name ($want)"
}

for file in "$@"; do
  upload "$file"
done

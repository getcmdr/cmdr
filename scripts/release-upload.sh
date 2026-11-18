#!/bin/bash
# Uploads files to a release and proves GitHub holds their exact bytes: four tries per file
# (GitHub's asset API fails often enough to cost a release; `--clobber` replaces a half-saved
# asset), then GitHub's SHA-256 of the asset must equal the local file's. The release pipeline's
# Windows publish job runs it (`release-pipeline.yml`, `windows-publish`); the macOS `sign` job
# carries the same loop inline, from before this script. Callers pass only files they own: the
# Windows publish job passes the names `release-assets.sh stage` derived from the manifest, so
# `--clobber` never replaces another job's asset.
#
# Usage: ./scripts/release-upload.sh <tag> <release id> <file>...
# Needs GH_TOKEN and GITHUB_REPOSITORY.
set -euo pipefail

TAG="$1"
RELEASE_ID="$2"
shift 2

# remove takes an asset this script couldn't prove off the release again, so nothing reads bytes
# that aren't the ones its caller built (and signed): a later `publish` then sees the file missing,
# not wrong. Best effort: if GitHub won't delete it either, the error above still fails the job.
remove() {
  gh release delete-asset "$TAG" "$1" -R "$GITHUB_REPOSITORY" -y \
    || echo "::error::Couldn't remove $1 from the release; delete it by hand before re-running."
}

upload() {
  local file="$1" name attempt want got
  name=$(basename "$file")
  for attempt in 1 2 3 4; do
    if gh release upload "$TAG" -R "$GITHUB_REPOSITORY" "$file" --clobber; then
      break
    fi
    if [ "$attempt" = 4 ]; then
      echo "::error::Couldn't upload $name after four tries."
      remove "$name"
      return 1
    fi
    sleep $((attempt * 15))
  done
  want="sha256:$(shasum -a 256 "$file" | cut -d' ' -f1)"
  # A fresh asset may not show its digest yet, so an empty one gets two more looks.
  for attempt in 1 2 3; do
    # The name goes in through the environment, never into the jq program's text, so no file
    # name can rewrite the query.
    got=$(NAME="$name" gh api "repos/$GITHUB_REPOSITORY/releases/$RELEASE_ID" \
      --jq '.assets[] | select(.name == env.NAME) | .digest // empty') || got=""
    [ -n "$got" ] && break
    sleep 5
  done
  if [ "$got" != "$want" ]; then
    echo "::error::$name on the release has digest '$got', but this job built $want, so it's coming off the release."
    remove "$name"
    return 1
  fi
  echo "Uploaded $name ($want)"
}

for file in "$@"; do
  upload "$file"
done

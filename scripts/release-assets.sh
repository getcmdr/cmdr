#!/bin/bash
# The release asset manifest's command line: the release pipeline's plan, `latest.json`, and the
# list of files `checksums.txt` covers, all from one list of what a release publishes. What it
# does: `scripts/release-assets/main.go`; the manifest: `scripts/release-assets/manifest/manifest.go`.
#
# Usage: ./scripts/release-assets.sh <plan|latest-json|checksum-files> -version X.Y.Z [flags]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Built, then run from the caller's directory, so relative paths in the flags mean what they say.
BIN="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/release-assets.XXXXXX")/release-assets"
(cd "${ROOT}/scripts/release-assets" && go build -o "$BIN" .)
# Where `latest-json` and `validate-latest` read the updater keys the app trusts.
RELEASE_ASSETS_ROOT="$ROOT" exec "$BIN" "$@"

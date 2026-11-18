#!/bin/bash
# Vets Windows NSIS installers for a release without running them: the stub's version resource, the
# app it packs, and its uninstall entry. What it checks: `scripts/release-nsis/nsis/nsis.go`.
#
# Usage: ./scripts/release-nsis.sh -version X.Y.Z Cmdr_X.Y.Z_x64-setup.exe...
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Built, then run from the caller's directory, so relative paths mean what they say.
BIN="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/release-nsis.XXXXXX")/release-nsis"
(cd "${ROOT}/scripts/release-nsis" && go build -o "$BIN" .)
# Where it reads the app's name and Windows publisher (the Tauri config).
RELEASE_NSIS_ROOT="$ROOT" exec "$BIN" "$@"

#!/bin/bash
# The Windows release's packages beside the NSIS installer: the Microsoft Store MSIX and the Group
# Policy template zip, from one compiled `Cmdr.exe`. What it does: `scripts/windows-package/main.go`.
#
# Usage: ./scripts/windows-package.sh <msix|verify-msix|group-policy> -version X.Y.Z [flags]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Built, then run from the caller's directory, so relative paths in the flags mean what they say.
BIN="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/windows-package.XXXXXX")/windows-package"
(cd "${ROOT}/scripts/windows-package" && go build -o "$BIN" .)
CMD="$1"
shift
exec "$BIN" "$CMD" -root "$ROOT" "$@"

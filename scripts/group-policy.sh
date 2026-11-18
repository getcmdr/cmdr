#!/bin/bash
# Generates the Group Policy template (ADMX and en-US ADML) from the app's policy definition, or
# with -check, fails when the committed files differ from it (`apps/desktop/packaging/group-policy/DETAILS.md`).
#
# Usage: ./scripts/group-policy.sh [-check]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}/scripts/group-policy"
exec go run . -root "${ROOT}" "$@"

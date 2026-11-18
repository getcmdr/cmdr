#!/bin/bash
# Rebuilds the NSIS fixtures `nsis_test.go` vets, with `makensis` (NSIS 3; `brew install makensis`
# on a Mac): a good installer of 0.70.0, one packing an app of another version, and one whose
# uninstall entry names another publisher. Run it from anywhere; it writes beside itself.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app() { # app <version> <publisher> <out>, an absolute path (makensis resolves paths from the script's folder)
  makensis -V2 -DVERSION="$1" -DPUBLISHER="$2" -DOUT="$3" payload.nsi > /dev/null
}
installer() { # installer <version> <publisher> <payload> <out>
  makensis -V2 -DPRODUCTNAME=Cmdr -DVERSION="$1" -DPUBLISHER="$2" -DPAYLOAD="$3" -DOUT="$PWD/$4" installer.nsi > /dev/null
}

app 0.70.0 "Test Publisher AB" "$work/Cmdr.exe"
mkdir -p "$work/old" && app 0.69.0 "Test Publisher AB" "$work/old/Cmdr.exe"
installer 0.70.0 "Test Publisher AB" "$work/Cmdr.exe" good-0.70.0-setup.exe
installer 0.70.0 "Test Publisher AB" "$work/old/Cmdr.exe" stale-app-0.70.0-setup.exe
installer 0.70.0 "Other Publisher AB" "$work/Cmdr.exe" other-publisher-0.70.0-setup.exe
ls -l ./*.exe

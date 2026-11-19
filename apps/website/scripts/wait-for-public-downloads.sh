#!/usr/bin/env bash
# Waits, before a website deploy, until the API server serves every OS download the new site will offer.
#
# The site and the API read one switch, `PUBLIC_DOWNLOAD_OSES` in `apps/api-server/wrangler.toml`, but deploy from
# separate workflows. Turning an OS on would otherwise put up buttons that answer the API's 404 until the Worker's
# deploy lands. So `ci.yml`'s `deploy-website` runs this first: for each public OS but macOS (always served) that the
# latest release carries a website file of (`apps/website/src/lib/download-platforms.ts`), it polls that file's
# download route until it redirects. A timeout fails the job, and the old site stays up.
# `apps/website/DETAILS.md` § Download platforms.
#
# The probe's User-Agent ends in `-bot`, so the API's bot filter serves it without writing a `downloads` row.
#
# Env overrides, for a local dry run: WRANGLER_TOML, MANIFEST_URL (file:// works), API_BASE, WAIT_SECONDS,
# POLL_SECONDS.
set -euo pipefail

root=$(cd "$(dirname "$0")/../../.." && pwd)
toml=${WRANGLER_TOML:-$root/apps/api-server/wrangler.toml}
manifest_url=${MANIFEST_URL:-https://github.com/getcmdr/cmdr/releases/latest/download/latest.json}
api=${API_BASE:-https://api.getcmdr.com}
wait_seconds=${WAIT_SECONDS:-900}
poll_seconds=${POLL_SECONDS:-20}

line=$(grep -E '^PUBLIC_DOWNLOAD_OSES *= *"[^"]*"' "$toml" || true)
if [ -z "$line" ]; then
  echo "::error::$toml has no PUBLIC_DOWNLOAD_OSES = \"…\" line, which the site builds from."
  exit 1
fi
oses=$(sed -E 's/^[^"]*"([^"]*)".*/\1/' <<<"$line" | tr ',' '\n' | tr -d ' ' | grep -Ev '^(macos)?$' || true)
if [ -z "$oses" ]; then
  echo "Only macOS is public, and the API always serves it: nothing to wait for."
  exit 0
fi

manifest=$(curl -fsSL --retry 3 "$manifest_url")
version=$(jq -r '.version' <<<"$manifest")

for os in $oses; do
  case $os in
    windows) kind=nsis ;;
    linux) kind=appimage ;;
    *)
      echo "Skipping $os: the site offers no download for it."
      continue
      ;;
  esac
  arch=$(jq -r --arg os "$os" --arg kind "$kind" \
    '[.downloads[]? | select(.os == $os and .kind == $kind) | .arch] | first // empty' <<<"$manifest")
  if [ -z "$arch" ]; then
    echo "$os is public, but $version carries no $kind, so the new site won't offer it: nothing to wait for."
    continue
  fi
  url="$api/download/$version/$os/$arch?kind=$kind"
  deadline=$((SECONDS + wait_seconds))
  while true; do
    status=$(curl -s -o /dev/null -w '%{http_code}' -A cmdr-deploy-check-bot "$url" || true)
    if [[ $status == 3* ]]; then
      echo "$url redirects ($status): the API serves $os."
      break
    fi
    if ((SECONDS >= deadline)); then
      echo "::error::$url still answers $status after ${wait_seconds}s, so this deploy stops and the old site stays up. Check the API server's deploy (deploy-api-server.yml)."
      exit 1
    fi
    echo "$url answers $status; waiting for the API server's deploy..."
    sleep "$poll_seconds"
  done
done

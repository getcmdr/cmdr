#!/bin/bash
# Provisions the two accounts the quota cells read. Runs as `www-data` in
# `/var/www/html`, from the Nextcloud image's `before-starting` hook: on EVERY
# container start, after any first-boot install and before apache binds a port.
#
# ❗ Both accounts exist for RFC 4331: Nextcloud answers `quota-available-bytes`
# with the negative sentinel `-3` for an unlimited account and with a real
# number for a limited one, and the backend reads those as different answers
# (`NotSupported` against a free-space figure). One account each is what makes
# both observable.
#
# ❗ Every boot, and so idempotent, because `/var/www/html` is a NAMED volume
# that outlives the container. A post-installation hook ran once per volume, so
# when the accounts were renamed (ada/grace → test_user_1/test_user_2,
# 2026-11-12), every machine's existing volume kept the old ones and both quota
# cells failed against a stack that looked healthy. Converging on each start
# means an edit here reaches a running fixture on its next recreation, which
# the stack lease triggers itself (this file is in the image's build context).
#
# ❗ A failure here stops the container (the image's `run_path` exits 1), so the
# readiness wait times out and `docker logs` names the step that broke.
set -e

QUOTA_USER="${QUOTA_USER:-test_user_1}"
UNLIMITED_USER="${UNLIMITED_USER:-test_user_2}"
USER_PASSWORD="${USER_PASSWORD:-openthedoor}"
# ❗ Exactly 5 GiB, and the cell asserts that number: `quota-available-bytes`
# plus `quota-used-bytes` has to add up to the ACCOUNT's quota rather than to
# the container's disk, which is what "does the free-space indicator show the
# right thing" comes down to.
QUOTA="${QUOTA:-5GB}"

# ❗ `user:add` refuses `openthedoor` while this app is on ("present in
# compromised password list"), and the fixture password is public on purpose.
# Nothing under test touches password policy: these cells read sabre/dav's
# answers to GET, PUT, and PROPFIND. A no-op (exit 0) once it's off.
php occ app:disable password_policy

# Creates the account, or resets its password to the fixture's if it exists
# (on a fresh volume, `QUOTA_USER` is the admin the install just made).
ensure_user() {
    if php occ user:info "$1" >/dev/null 2>&1; then
        OC_PASS="$USER_PASSWORD" php occ user:resetpassword --password-from-env "$1"
    else
        OC_PASS="$USER_PASSWORD" php occ user:add --password-from-env "$1"
    fi
}

ensure_user "$QUOTA_USER"
php occ user:setting "$QUOTA_USER" files quota "$QUOTA"

# Explicitly unlimited rather than left at the instance default, so the
# sentinel is a decision this file makes and not one a future default quota
# could take away.
ensure_user "$UNLIMITED_USER"
php occ user:setting "$UNLIMITED_USER" files quota none

echo "cmdr fixture: provisioned $QUOTA_USER (quota $QUOTA) and $UNLIMITED_USER (unlimited)"
php occ user:list

/**
 * Network settings helper - provides computed values for network operations.
 * Centralizes the logic for calculating timeout and cache values from settings.
 */

import { getSetting } from './settings-store'

/**
 * Gets the network timeout in milliseconds: how long a network operation waits before giving up.
 * Uses the network.timeout setting, which is stored in milliseconds and edited in seconds.
 */
export function getNetworkTimeoutMs(): number {
  return getSetting('network.timeout')
}

/**
 * Gets the mount timeout in milliseconds.
 * Uses the advanced.mountTimeout setting which is stored in milliseconds.
 */
export function getMountTimeoutMs(): number {
  return getSetting('advanced.mountTimeout')
}

/**
 * Gets the share cache TTL (Time To Live) in milliseconds.
 * Uses the network.shareCacheDuration setting which is stored in milliseconds.
 */
export function getShareCacheTtlMs(): number {
  return getSetting('network.shareCacheDuration')
}

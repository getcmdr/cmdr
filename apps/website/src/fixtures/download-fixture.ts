/**
 * A frozen release that ships every OS, for `/download-fixture` and its E2E: the download section's "shipped" state
 * for Windows and Linux, which the real build can't show until those builds are public. Placeholder version and
 * sizes; links point at a host that doesn't exist, so nothing here can be downloaded by accident.
 */
import { offeredPlatforms, type Os } from '../lib/download-platforms'
import type { UpdateManifest } from '../lib/release-downloads'

const mb = 1024 * 1024

export const fixtureManifest: UpdateManifest = {
  version: '9.9.0',
  downloads: [
    { os: 'macos', arch: 'aarch64', kind: 'dmg', file: 'Cmdr_9.9.0_aarch64.dmg', size: 50 * mb },
    { os: 'macos', arch: 'x86_64', kind: 'dmg', file: 'Cmdr_9.9.0_x64.dmg', size: 53 * mb },
    { os: 'macos', arch: 'universal', kind: 'dmg', file: 'Cmdr_9.9.0_universal.dmg', size: 91 * mb },
    { os: 'windows', arch: 'x86_64', kind: 'nsis', file: 'Cmdr_9.9.0_x64-setup.exe', size: 21 * mb },
    { os: 'windows', arch: 'x86_64', kind: 'msix', file: 'Cmdr_9.9.0_x64.msix', size: 30 * mb },
    { os: 'linux', arch: 'x86_64', kind: 'appimage', file: 'Cmdr_9.9.0_amd64.AppImage', size: 80 * mb },
    { os: 'linux', arch: 'aarch64', kind: 'appimage', file: 'Cmdr_9.9.0_aarch64.AppImage', size: 78 * mb },
    { os: 'linux', arch: 'x86_64', kind: 'deb', file: 'Cmdr_9.9.0_amd64.deb', size: 25 * mb },
    { os: 'linux', arch: 'aarch64', kind: 'deb', file: 'Cmdr_9.9.0_arm64.deb', size: 24 * mb },
  ],
}

export const fixturePlatforms = offeredPlatforms(
  fixtureManifest,
  new Set<Os>(['macos', 'windows', 'linux']),
  (d) => `https://downloads.example.invalid/v9.9.0/${d.file}`,
)

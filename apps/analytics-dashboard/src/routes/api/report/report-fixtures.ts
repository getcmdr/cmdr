/**
 * The report goldens' inputs (`format-report.test.ts`), built to hit the branches that matter: a section
 * with data, a section whose source is down, a source that loaded but is empty, and the sections that
 * read two sources and lose only one. The funnel fixture mixes `null` (unknown, renders as `-`) with a
 * real `0` on the same day, because those mean different things everywhere in the dashboard. "Today" is
 * `updatedAt`'s UTC day (2026-08-02): partial, so the Product lines read yesterday and print today only
 * as a marked line.
 */
import type { DashboardData } from '../../../lib/server/fetch-all.js'
import type { FeatureUsageData, FeatureUsagePeriods } from '../../../lib/feature-usage.js'
import type { ActiveInstallsData } from '../../../lib/server/sources/active-installs.js'
import type { SettingAdoption } from '../../../lib/server/settings-defaults.js'
import type { ErrorReportRow, FeedbackRow } from '../../../lib/feedback-and-errors.js'

const site = (pv: number, prevPv: number, visitors: number, prevVisitors: number) => ({
  pageviews: { value: pv, prev: prevPv },
  visitors: { value: visitors, prev: prevVisitors },
  visits: { value: visitors, prev: prevVisitors },
  bounces: { value: Math.floor(visitors / 2), prev: 0 },
  totaltime: { value: 0, prev: 0 },
})

/** Every source loaded, with enough shape to reach each breakdown. */
const richFeatureUsage: FeatureUsageData = {
  range: '90d',
  since: '2026-05-04T00:00:00.000Z',
  until: '2026-08-02T00:00:00.000Z',
  activeInstalls: 40,
  features: [
    {
      id: 'tabs',
      name: 'Tabs',
      sources: ['tab_opened'],
      installs: 0,
      events: 0,
      everSeen: true,
      firstSeenAt: '2026-01-01T00:00:00.000Z',
      parts: [],
    },
    {
      id: 'ask-cmdr',
      name: 'Ask Cmdr',
      sources: ['ask_cmdr_turn'],
      installs: 3,
      events: 12,
      everSeen: true,
      // First stored after the previous window began: its zero there means "not stored yet", so no change.
      firstSeenAt: '2026-07-01T00:00:00.000Z',
      parts: [
        { key: 'answered', installs: 3, events: 10 },
        { key: 'failed', installs: 1, events: 2 },
      ],
    },
    {
      id: 'multi-rename',
      name: 'Multi-rename tool',
      sources: ['multi_rename_applied'],
      installs: 0,
      events: 0,
      everSeen: false,
      firstSeenAt: null,
      parts: [],
    },
    {
      id: 'archives',
      name: 'Archives',
      sources: ['pane_navigated[volume_kind=archive]', 'archive_edit_completed'],
      installs: 30,
      events: 4_321,
      everSeen: true,
      firstSeenAt: '2026-01-01T00:00:00.000Z',
      parts: [{ key: 'pane_navigated', installs: 30, events: 4_321 }],
    },
    {
      id: 'git-browser',
      name: 'Git browser',
      sources: ['pane_navigated[volume_kind=git-portal]'],
      installs: 1,
      events: 1,
      everSeen: true,
      firstSeenAt: '2026-01-01T00:00:00.000Z',
      parts: [],
    },
  ],
}

/** The 90 days before: archives went from half to three quarters of active installs. */
const richFeatureUsagePeriods: FeatureUsagePeriods = {
  current: richFeatureUsage,
  previous: {
    ...richFeatureUsage,
    since: '2026-02-03T00:00:00.000Z',
    until: '2026-05-04T00:00:00.000Z',
    activeInstalls: 20,
    features: richFeatureUsage.features.map((f) => (f.id === 'archives' ? { ...f, installs: 10 } : f)),
  },
}

/**
 * The heartbeat: a day 90 days before yesterday (the point tiles' comparison), the last two complete
 * days around the v1.10.0 release, and today. The 90-day window has a version with too few installs
 * for an error rate.
 */
const richActiveInstalls: ActiveInstallsData = {
  days: [
    { date: '2026-05-03', dau: 20, newInstalls: 2, appHours: 100, wau: 40, mau: 50, byVersion: { '1.2.0': 20 } },
    { date: '2026-07-31', dau: 40, newInstalls: 5, appHours: 320.4, wau: 60, mau: 80, byVersion: { '1.2.0': 40 } },
    {
      date: '2026-08-01',
      dau: 55,
      newInstalls: 9,
      appHours: 500,
      wau: 70,
      mau: 90,
      byVersion: { '1.10.0': 45, '1.2.0': 10 },
    },
    { date: '2026-08-02', dau: 12, newInstalls: 1, appHours: 30, wau: 72, mau: 91, byVersion: { '1.10.0': 12 } },
  ],
  windows: [
    {
      days: 90,
      since: '2026-05-04',
      until: '2026-08-01',
      installs: 60,
      byVersion: { '1.10.0': 45, '1.2.0': 50, '1.1.0': 3 },
    },
  ],
}

/** A preference whose default moved, plus the state flags the Onboarding section and the Money card read. */
const richStateSettings: SettingAdoption[] = [
  {
    key: 'ui.density',
    kind: 'preference',
    eligible: 26,
    onDefault: 21,
    defaultLabel: null,
    latestDefaultLabel: 'compact',
    defaultMovedIn: '1.10.0',
    values: [
      { label: 'compact', installs: 21, isDefault: true },
      { label: 'roomy', installs: 5, isDefault: false },
    ],
  },
  {
    key: 'onboarding.completed',
    kind: 'state',
    eligible: 26,
    onDefault: 6,
    defaultLabel: 'false',
    latestDefaultLabel: 'false',
    defaultMovedIn: null,
    values: [
      { label: 'true', installs: 20, isDefault: false },
      { label: 'false', installs: 6, isDefault: true },
    ],
  },
  {
    key: 'fileExplorer.quickFilterIntroSeen',
    kind: 'state',
    eligible: 26,
    onDefault: 16,
    defaultLabel: 'false',
    latestDefaultLabel: 'false',
    defaultMovedIn: null,
    values: [
      { label: 'false', installs: 16, isDefault: true },
      { label: 'true', installs: 10, isDefault: false },
    ],
  },
  {
    // Capability-gated and young: too few eligible installs for a share.
    key: 'behavior.textEditorHintSeen',
    kind: 'state',
    eligible: 4,
    onDefault: 3,
    defaultLabel: 'false',
    latestDefaultLabel: 'false',
    defaultMovedIn: null,
    values: [
      { label: 'false', installs: 3, isDefault: true },
      { label: 'true', installs: 1, isDefault: false },
    ],
  },
  {
    key: 'behavior.adbHintDismissed',
    kind: 'state',
    eligible: 10,
    onDefault: 9,
    defaultLabel: 'false',
    latestDefaultLabel: 'false',
    defaultMovedIn: null,
    values: [
      { label: 'false', installs: 9, isDefault: true },
      { label: 'true', installs: 1, isDefault: false },
    ],
  },
  {
    key: 'onboarding.upgradeNudgeShown',
    kind: 'state',
    eligible: 3,
    onDefault: 1,
    defaultLabel: 'false',
    latestDefaultLabel: 'false',
    defaultMovedIn: null,
    values: [
      { label: 'true', installs: 2, isDefault: false },
      { label: 'false', installs: 1, isDefault: true },
    ],
  },
]

const richFeedback: FeedbackRow[] = [
  {
    id: 1,
    createdAt: '2026-08-01 12:33:44',
    feedback: 'Love   it,\n  especially the\tpreview pane.',
    email: 'someone@example.com',
    appVersion: '1.10.0',
    osVersion: '15.5',
    buildMode: 'release',
    answeredAt: null,
    errorReportId: null,
  },
  {
    id: 2,
    createdAt: '2026-07-31 08:00:00',
    // Longer than the 280-character cut-off.
    feedback: 'x'.repeat(300),
    email: null,
    appVersion: '1.2.0',
    osVersion: '14.7',
    buildMode: null,
    answeredAt: null,
    errorReportId: 'ERR-AB7PV',
  },
  {
    id: 3,
    createdAt: '2026-07-30 10:00:00',
    feedback: 'Already got a reply.',
    email: 'answered@example.com',
    appVersion: '1.2.0',
    osVersion: '14.7',
    buildMode: 'release',
    answeredAt: '2026-07-30 18:45:00',
    errorReportId: null,
  },
]

/** Older than the range, still owed a reply: counted and listed whatever the range. */
const oldAwaiting: FeedbackRow = {
  id: 4,
  createdAt: '2026-04-01 09:00:00',
  feedback: 'Still waiting on this one.',
  email: 'waiting@example.com',
  appVersion: '1.1.0',
  osVersion: '14.7',
  buildMode: 'release',
  answeredAt: null,
  errorReportId: null,
}

const richErrorReports: ErrorReportRow[] = [
  {
    id: 'er_1',
    kind: 'auto',
    appVersion: '1.10.0',
    osVersion: '15.5',
    arch: 'aarch64',
    date: '2026-08-01',
    generatedAt: '2026-08-01T12:00:00Z',
  },
  {
    id: 'er_2',
    kind: 'auto',
    appVersion: '1.2.0',
    osVersion: '14.7',
    arch: 'x86_64',
    date: '2026-07-31',
    generatedAt: '2026-07-31T12:00:00Z',
  },
  {
    id: 'er_3',
    kind: 'user',
    appVersion: '1.10.0',
    osVersion: '15.5',
    arch: 'aarch64',
    date: '2026-08-01',
    generatedAt: '2026-08-01T13:00:00Z',
  },
]

export const richData: DashboardData = {
  selection: { range: '90d', end: null },
  updatedAt: '2026-08-02T03:04:05.000Z',
  funnel: {
    ok: true,
    data: {
      rows: [
        {
          date: '2026-07-30',
          visitors: 100,
          downloadClicks: 10,
          serverDownloads: 8,
          downloadsByRef: { twitter: 5, '(none)': 3 },
          downloadsByReferer: { 'news.ycombinator.com': 4, '(none)': 4 },
          downloadsByUaFamily: { human: 6, bot: 1, unknown: 1 },
          humanInstalls: 7,
          newInstalls: 4,
          d7Retention: 0.5,
          d7Retained: 2,
          newsletterSignups: 3,
          purchases: 1,
          pageviews: null,
          downloadsBySource: null,
          newInstallsByChannel: null,
          retentionSoFar: null,
          d28Retention: null,
          d28Retained: null,
          d28SoFar: null,
          byCountry: {},
        },
        {
          // A day where the web side is unknown (null) but the server side is a real zero.
          date: '2026-07-31',
          visitors: null,
          downloadClicks: 0,
          serverDownloads: 0,
          downloadsByRef: {},
          downloadsByReferer: null,
          downloadsByUaFamily: null,
          humanInstalls: null,
          newInstalls: 0,
          d7Retention: null,
          d7Retained: null,
          newsletterSignups: null,
          purchases: 0,
          pageviews: null,
          downloadsBySource: null,
          newInstallsByChannel: null,
          retentionSoFar: null,
          d28Retention: null,
          d28Retained: null,
          d28SoFar: null,
          byCountry: {},
        },
        {
          date: '2026-08-01',
          visitors: 250,
          downloadClicks: 25,
          serverDownloads: 20,
          downloadsByRef: { twitter: 10, reddit: 5 },
          downloadsByReferer: { 'news.ycombinator.com': 6 },
          downloadsByUaFamily: { human: 15, bot: 3, unknown: 2 },
          humanInstalls: 17,
          newInstalls: 9,
          d7Retention: 0.333,
          d7Retained: 3,
          newsletterSignups: 2,
          purchases: 2,
          pageviews: null,
          downloadsBySource: null,
          newInstallsByChannel: null,
          retentionSoFar: null,
          d28Retention: null,
          d28Retained: null,
          d28SoFar: null,
          byCountry: {},
        },
      ],
      countries: { top: [], extra: null, umami: false, worker: false, breakdowns: false },
      workerStatus: 'ok',
    },
  },
  umami: {
    ok: true,
    data: {
      personalSite: site(1200, 1000, 800, 900),
      // A zero `prev` is the "no prior period" case: the delta suffix drops out entirely.
      website: site(5400, 5400, 3200, 0),
      prvw: site(120, 0, 90, 45),
      websiteReferrers: [
        { x: 'news.ycombinator.com', y: 900 },
        { x: '', y: 400 },
      ],
      websitePages: [
        { x: '/', y: 3000 },
        { x: '/pricing', y: 700 },
      ],
      websiteCountries: [
        { x: 'US', y: 1800 },
        { x: 'SE', y: 600 },
        // Not a region code: `formatCountry` has to fall back to the raw key.
        { x: '(unknown)', y: 100 },
      ],
      downloadEvents: [{ x: 'download-mac', y: 240 }],
      prvwReferrers: [{ x: 'google.com', y: 60 }],
      prvwPages: [{ x: '/', y: 120 }],
      personalSiteReferrers: [{ x: 'linkedin.com', y: 30 }],
    },
  },
  cloudflare: {
    ok: true,
    data: {
      downloads: [
        {
          version: '1.2.0',
          arch: 'aarch64',
          country: 'US',
          source: 'website',
          day: '2026-07-31',
          downloads: 40,
          uniqueDownloads: 30,
        },
        {
          version: '1.2.0',
          arch: 'x86_64',
          country: 'US',
          source: 'homebrew',
          day: '2026-07-31',
          downloads: 12,
          uniqueDownloads: 9,
        },
        {
          // 1.10.0 sorts above 1.2.0 only under semver comparison, never lexically.
          version: '1.10.0',
          arch: 'aarch64',
          country: 'SE',
          source: 'website',
          day: '2026-08-01',
          downloads: 25,
          uniqueDownloads: 20,
        },
        {
          version: '1.10.0',
          arch: 'aarch64',
          country: '(unknown)',
          source: 'other',
          day: '2026-08-01',
          downloads: 3,
          uniqueDownloads: 3,
        },
      ],
      updateActivity: [
        // The reach beside the daily actives: above them on 2026-08-01, below on 2026-07-31 (NAT, or a
        // check that didn't fire), and no data at all for today. All three are real shapes.
        { day: '2026-07-31', version: '1.2.0', updaters: 30 },
        { day: '2026-08-01', version: '1.10.0', updaters: 50 },
        { day: '2026-08-01', version: '1.2.0', updaters: 12 },
      ],
    },
  },
  activeInstalls: { ok: true, data: richActiveInstalls },
  paddle: {
    ok: true,
    data: {
      allTransactions: [
        {
          // A first purchase in the previous window: the USD change reads against it.
          id: 'txn_0',
          status: 'completed',
          createdAt: '2026-03-01T10:00:00Z',
          total: '3900',
          currencyCode: 'USD',
          origin: 'web',
          payout: null,
        },
        {
          // A renewal: revenue, but not a paid license and not in the purchases list. Its currency is
          // new this window, so its change has nothing to compare against.
          id: 'txn_1',
          status: 'completed',
          createdAt: '2026-07-31T09:00:00Z',
          total: '8900',
          currencyCode: 'EUR',
          origin: 'subscription_recurring',
          payout: null,
        },
        {
          id: 'txn_2',
          status: 'completed',
          createdAt: '2026-08-01T10:00:00Z',
          total: '3900',
          currencyCode: 'USD',
          origin: 'web',
          payout: null,
        },
        {
          // Today: listed, marked, and in no stat.
          id: 'txn_3',
          status: 'completed',
          createdAt: '2026-08-02T01:30:00Z',
          total: '3900',
          currencyCode: 'USD',
          origin: 'api',
          payout: null,
        },
      ],
    },
  },
  github: {
    ok: true,
    data: {
      totalDownloads: 1234,
      releases: [
        {
          tagName: 'v1.10.0',
          publishedAt: '2026-08-01T08:00:00Z',
          draft: false,
          prerelease: false,
          assets: [],
          totalDownloads: 800,
        },
        {
          tagName: 'v1.2.0',
          publishedAt: '2026-07-01T08:00:00Z',
          draft: false,
          prerelease: false,
          assets: [],
          totalDownloads: 434,
        },
      ],
    },
  },
  githubStars: {
    ok: true,
    data: {
      totalStars: 310,
      repos: [
        {
          repo: 'getcmdr/cmdr',
          // One star day inside 7d, one inside 30d only, one outside both.
          totalStars: 300,
          daily: [
            { day: '2026-01-01', newStars: 250, cumulative: 250 },
            { day: '2026-07-20', newStars: 40, cumulative: 290 },
            { day: '2026-08-01', newStars: 10, cumulative: 300 },
          ],
        },
        { repo: 'vdavid/mtp-rs', totalStars: 10, daily: [{ day: '2026-01-01', newStars: 10, cumulative: 10 }] },
      ],
      combinedDaily: [],
    },
  },
  posthog: {
    ok: true,
    data: {
      totalPageviews: 900,
      dailyPageviews: [
        { day: '2026-07-31', views: 400 },
        { day: '2026-08-01', views: 500 },
      ],
    },
  },
  license: { ok: true, data: { totalActivations: 42, activeDevices: 7 } },
  settingsAdoption: {
    ok: true,
    data: {
      totalInstalls: 30,
      unresolvedInstalls: 4,
      settings: [
        {
          key: 'ai.provider',
          kind: 'preference',
          eligible: 26,
          onDefault: 20,
          defaultLabel: 'off',
          latestDefaultLabel: 'off',
          defaultMovedIn: null,
          values: [
            { label: 'off', installs: 20, isDefault: true },
            { label: 'cloud', installs: 6, isDefault: false },
          ],
        },
        {
          key: 'indexing.enabled',
          kind: 'preference',
          eligible: 26,
          onDefault: 24,
          defaultLabel: 'on',
          latestDefaultLabel: 'on',
          defaultMovedIn: null,
          values: [
            { label: 'on', installs: 24, isDefault: true },
            { label: 'off', installs: 2, isDefault: false },
          ],
        },
        {
          // Younger than the fleet, so its total is smaller on purpose.
          key: 'mediaIndex.enabled',
          kind: 'preference',
          eligible: 10,
          onDefault: 8,
          defaultLabel: 'off',
          latestDefaultLabel: 'off',
          defaultMovedIn: null,
          values: [
            { label: 'off', installs: 8, isDefault: true },
            { label: 'on', installs: 2, isDefault: false },
          ],
        },
        {
          key: 'theme.mode',
          kind: 'preference',
          eligible: 26,
          onDefault: 26,
          defaultLabel: 'system',
          latestDefaultLabel: 'system',
          defaultMovedIn: null,
          values: [{ label: 'system', installs: 26, isDefault: true }],
        },
        ...richStateSettings,
      ],
    },
  },
  feedbackAndErrors: {
    ok: true,
    data: {
      feedback: richFeedback,
      awaitingReply: [richFeedback[0], oldAwaiting],
      allErrorReports: [
        ...richErrorReports,
        {
          // Outside the picker's range, inside the 90-day version window: only the rate counts it.
          id: 'er_4',
          kind: 'auto',
          appVersion: '1.1.0',
          osVersion: '14.7',
          arch: 'aarch64',
          date: '2026-07-01',
          generatedAt: '2026-07-01T12:00:00Z',
        },
      ],
      errorReports: richErrorReports,
    },
  },
  featureUsage: { ok: true, data: richFeatureUsagePeriods },
}

/** Every source down. */
export const allFailedData: DashboardData = {
  selection: { range: '7d', end: null },
  updatedAt: '2026-08-02T03:04:05.000Z',
  funnel: { ok: false, error: 'Funnel: timed out after 20s' },
  umami: { ok: false, error: 'Umami: not configured (missing env vars)' },
  cloudflare: { ok: false, error: 'Cloudflare: timed out after 20s' },
  activeInstalls: { ok: false, error: 'Active installs: timed out after 20s' },
  paddle: { ok: false, error: 'Paddle: not configured (missing env vars)' },
  github: { ok: false, error: 'GitHub: offline' },
  githubStars: { ok: false, error: 'GitHub stars: offline' },
  posthog: { ok: false, error: 'PostHog: not configured (missing env vars)' },
  license: { ok: false, error: 'License server: not configured (missing env vars)' },
  feedbackAndErrors: { ok: false, error: 'Feedback & errors: timed out after 20s' },
  settingsAdoption: { ok: false, error: 'Settings adoption: timed out after 20s' },
  featureUsage: { ok: false, error: 'Feature usage: timed out after 20s' },
}

/** Every source loaded, every collection empty: the "present but nothing in it" shape. */
export const emptyData: DashboardData = {
  selection: { range: '30d', end: null },
  updatedAt: '2026-08-02T03:04:05.000Z',
  funnel: {
    ok: true,
    data: {
      rows: [],
      countries: { top: [], extra: null, umami: false, worker: false, breakdowns: false },
      workerStatus: 'ok',
    },
  },
  umami: {
    ok: true,
    data: {
      personalSite: site(0, 0, 0, 0),
      website: site(0, 0, 0, 0),
      prvw: site(0, 0, 0, 0),
      websiteReferrers: [],
      websitePages: [],
      websiteCountries: [],
      downloadEvents: [],
      prvwReferrers: [],
      prvwPages: [],
      personalSiteReferrers: [],
    },
  },
  cloudflare: { ok: true, data: { downloads: [], updateActivity: [] } },
  activeInstalls: { ok: true, data: { days: [], windows: [] } },
  paddle: { ok: true, data: { allTransactions: [] } },
  github: { ok: true, data: { totalDownloads: 0, releases: [] } },
  githubStars: { ok: true, data: { totalStars: 0, repos: [], combinedDaily: [] } },
  posthog: { ok: true, data: { totalPageviews: 0, dailyPageviews: [] } },
  license: { ok: true, data: { totalActivations: 0, activeDevices: null } },
  feedbackAndErrors: { ok: true, data: { feedback: [], errorReports: [], awaitingReply: [], allErrorReports: [] } },
  settingsAdoption: { ok: true, data: { settings: [], totalInstalls: 0, unresolvedInstalls: 0 } },
  // An api-server that predates complete-day windows: no `until`, no previous window.
  featureUsage: {
    ok: true,
    data: {
      current: {
        range: '30d',
        since: '2026-07-03T03:04:05.000Z',
        activeInstalls: 0,
        features: [
          { id: 'search', name: 'Search', sources: ['search_used'], installs: 0, events: 0, everSeen: true, parts: [] },
        ],
      },
      previous: null,
    },
  },
}

/**
 * Half the sources down: the sections that read two sources have to degrade one at a time. The api-server
 * predates `/admin/active-installs` (`data: null`), so the heartbeat lines read "Not available yet".
 */
export const mixedData: DashboardData = {
  selection: { range: '7d', end: null },
  updatedAt: '2026-08-02T03:04:05.000Z',
  funnel: { ok: false, error: 'Funnel: offline' },
  umami: { ok: false, error: 'Umami: offline' },
  cloudflare: { ok: false, error: 'Cloudflare: offline' },
  activeInstalls: { ok: true, data: null },
  paddle: richData.paddle,
  github: richData.github,
  githubStars: { ok: false, error: 'GitHub stars: offline' },
  posthog: richData.posthog,
  license: { ok: false, error: 'License server: offline' },
  feedbackAndErrors: {
    ok: true,
    data: {
      feedback: [],
      errorReports: richErrorReports,
      awaitingReply: [],
      allErrorReports: richErrorReports,
    },
  },
  settingsAdoption: { ok: false, error: 'Settings adoption: offline' },
  featureUsage: { ok: false, error: 'Feature usage: offline' },
}

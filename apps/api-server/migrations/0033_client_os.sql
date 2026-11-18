-- The OS a row is about, now that Cmdr builds for more than macOS (`docs/specs/windows-port-plan.md`
-- task 31). Every change here is additive, so a Worker from before this migration keeps writing
-- correct rows in the window between `migrations apply` and `deploy`.
--
-- 1. `downloads.os` / `downloads.kind`: the release file a `/download` hit fetched (`macos` + `dmg`,
--    later `windows` + `nsis`, `linux` + `appimage`, ...), from `GET /download/:version/:os/:arch`.
--    NOT NULL with the macOS defaults because every row before this, and every row an older Worker
--    writes, is a macOS DMG: the old route serves nothing else. Adding a column with a constant
--    default is a schema-only change in SQLite, so the big table isn't rewritten.
--
-- 2. `downloads_daily_unique` gains `os` in its UNIQUE key. The rollup's grouping must match what
--    `/admin/downloads` groups by (a distinct count isn't additive), and a Mac and a Windows
--    download of `x86_64` on one day from one country would otherwise collide and lose one count.
--    A UNIQUE constraint can't be altered, so the table is rebuilt; its existing rows are all macOS.
--    The old Worker's sweep inserts without `os` and gets the default, which is right for it.
--
-- 3. `os` on `heartbeat`, `feedback`, `crash_reports`, and `error_reports`: the `os` field the app
--    sends (`macos` | `windows` | `linux`). Nullable: older clients don't send it, and the readers
--    fall back to the row's `os_version` (`src/client-os.ts::osOfRow`), so nothing is backfilled.

ALTER TABLE downloads ADD COLUMN os TEXT NOT NULL DEFAULT 'macos';
ALTER TABLE downloads ADD COLUMN kind TEXT NOT NULL DEFAULT 'dmg';

CREATE TABLE downloads_daily_unique_by_os (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT NOT NULL,
    app_version TEXT NOT NULL,
    os TEXT NOT NULL DEFAULT 'macos',
    arch TEXT NOT NULL,
    country TEXT NOT NULL,
    source TEXT NOT NULL,
    unique_downloaders INTEGER NOT NULL,
    UNIQUE(date, app_version, os, arch, country, source)
);

INSERT INTO downloads_daily_unique_by_os (date, app_version, os, arch, country, source, unique_downloaders)
    SELECT date, app_version, 'macos', arch, country, source, unique_downloaders FROM downloads_daily_unique;

DROP TABLE downloads_daily_unique;
ALTER TABLE downloads_daily_unique_by_os RENAME TO downloads_daily_unique;
CREATE INDEX idx_downloads_daily_unique_date ON downloads_daily_unique(date);

ALTER TABLE heartbeat ADD COLUMN os TEXT;
ALTER TABLE feedback ADD COLUMN os TEXT;
ALTER TABLE crash_reports ADD COLUMN os TEXT;
ALTER TABLE error_reports ADD COLUMN os TEXT;

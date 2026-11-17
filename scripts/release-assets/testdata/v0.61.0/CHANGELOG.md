## [0.61.0] - 2026-11-13

More fixes than new things this time. The most interesting new things:

- Copying thousands of small files onto a local disk got many times faster. It's always a compromise between speed and
  data safety, but the old balance was way too conservative. Transfers are now a lot faster, and Cmdr still makes sure
  every file is safely on the drive before it reports "done".
- The Operation log got a lot more useful: if you did something silly, you can now properly review and roll back your
  copy, move, or trash operation, in whole or in part. Even days later.
- If a volume suddenly becomes unavailable mid-transfer (typically a NAS reboot or a router restart), the file now
  carries on from where it was once the share is back: this applies to downloads from SMB, SFTP, NFS, S3, and WebDAV,
  and to uploads to SMB.

Plus ~50 smaller fixes and improvements.

### Added

- Roll back only the items you check: the Operation log is now a list of operations beside a checkbox tree of each one's
  items, with "Roll back selected", "Roll back the rest", and a key for every action (8583f7b78, dbfcef1e8, 1f39a2bdb,
  1a243a0fb, e318e6efa, 329e2a291, 64e2f6a5a, 4a5b1bc36, c0db1021b, 360ba5e27, f02e6efd2, c952d563e)
- Carry a file on from where it stopped after a long outage instead of starting it over, for downloads from SMB, SFTP,
  NFS, S3, and WebDAV, and for uploads to SMB (1e7b71244, 91a062216, b354704f0, a901fb20d, 558ee26c4, 09667a936,
  7a36d86fd, 412d46bcc, 9dd2a7d2b, dc4e59113)
- Drop an image from Chrome, Safari, or Mail into Cmdr to copy it (83e1a83fc, 3eb8cb5ca)
- Copy from cameras and other USB devices that can't read a file in parts (a2d474e3c)
- Load more results as you scroll in Search and Selection lists, up to 10,000 (1459e8bd1)
- Say how many hidden items an empty-looking folder holds, and how to show them (70ec66b3e, c0aa42b2f, 21ad6c894,
  00ec5d102, dc74d7f45)
- Go back and forward with ⌥← and ⌥→ (1df7208a9)
- Jump to the first and last row with ← and → in a search results pane (3ec3190b9)
- Give Linux its own keyboard: Total Commander-style default keys, Linux key names everywhere, a real Quit, F11 full
  screen, working AltGr and text-editing keys, and a warning for keys your desktop takes (1df7208a9, 2988adb5f,
  0b4f684c9, 3d0df12a4, e4b7a4675, 47351fb70, 3fd18a71f, 4a8cdf391, 4dac271f5, 3e4869f7b, df7327dc9, 325957c3e,
  130f152a4, e58bbff3e, 60975d7eb, 1a717c31a, feb2c26a5, 6392a4a21, 52b964b3a)

### Changed

- Copy and move thousands of small files onto a local disk many times faster (792e05f9e, 112cdd91d, 83fcab2c8,
  c2604b4c8)
- Keep the viewer and running copies going through a dropped SMB connection, and reload a viewed file that changed
  meanwhile (9dd2a7d2b, 16bce5d05)
- Let a key you bound yourself win over a default, and show only keys that really run their command (48063e2b4,
  d07a89b8b)
- Show every shortcut in a message as a key cap with your own key, or name the command if it has none (92f22a8ad,
  4873d1659, d2751f93b, ce125e7e9, 2bdcb08a1, 5a6b808da)
- Show shortcut modifiers in the menu bar's order, like ⇧⌘P (09e2f4fab)
- Move the search dialog's recent searches from ⌘H to ⌘Y, and list the search and viewer keys in Settings (4a8cdf391)
- Slide Settings, the viewer, and other secondary windows in and out (bdb7efa7a)
- Select the cursor row too on the first ⌘-click, like Finder (3a90c8d06)
- Say each refused folder list in one line under a search, and offer Full Disk Access only where it helps (ff2b5770d,
  46b725f1e, 01fe8cf17)
- Say a file is open somewhere else when a share or the viewer can't read it (242167bcd, d01074507, e92e40f17,
  a4c65bfb6)
- Explain a refused trash when another account owns the items or the drive's trash won't take them (8c42c2074,
  38649d6fa)
- Keep browsing a phone responsive during a copy, on any cable (6d3b1ec96)
- Mark an operation a crash cut off as "Didn't finish", rollbackable as far as its record proves (d5514dda1, 95a8dfb29,
  a20e81157)
- Word everything the Linux way on Linux (file manager, keyring, Unmount, this computer), and hide Mac-only features
  there (2445e477a, 7d48eec76, e6af3cfb6, 413a622b9, a459057f0, 36bb663b4, 2253c8744, 4b81eb542, 69c255599, 52f120913,
  3f6390e2a, 0f042d9c5, 838f65a7a, b95f6ae58, 218e8d952, 42c1fbe71)
- Stop runaway indexing memory on Linux too (94ee5c9f2)
- Graduate nine features to stable and eight to beta, SMB shares and Android phones among them (16b5f4eda)
- Include about 20 behavior settings in error reports, with no paths or names (2fbf98c59)
- Count six more features and the install channel (Homebrew or direct) in the anonymous usage stats (75fe0425a,
  bac18f749)

### Fixed

- Fix a move between drives deleting the originals after the destination drive was unplugged (796c2ee87)
- Fix ⌘⌫, F8, ⌥⌘V, and other file-moving keys acting on your files while you type in a text field, like the path bar or
  a rename (e434a2b98, 40e900023)
- Fix ⌘↑ in a text field going to the parent folder instead of the start of the text (9a615ebb8)
- Fix letter shortcuts going dead on Russian, Greek, Hebrew, and other non-Latin keyboard layouts (c40073159, e8e50f802)
- Fix a second Cmdr icon in the Dock after Restart on an update (37d145940)
- Fix "Show in Finder" from Firefox, Chrome, and App Store apps only bringing Cmdr forward (5f6e57137)
- Fix a copy's time left jumping to hours on a run of tiny files or behind a big one (99967db14, fa2a9189f)
- Fix a failed copy from a Dropbox-style folder blaming a disconnected drive (6e00aed63)
- Fix deleting something already gone from a share ending in an error (63fd1dabb)
- Fix the queue showing a finished operation as still running (f721d6c79)
- Fix a crash mid-rollback making a partly rolled-back operation look untouched (876e196ba, df455f86e)
- Fix an old rollback reappearing in the Operation log as something to roll back (51a44d924)
- Fix two rollbacks of one operation running at once (0694533c5)
- Fix the Search dialog closing when a file showed up twice in the index (2b8aaa683)
- Fix "Show all in main window" opening only 30 results under a toast that never ended (e683db8f4)
- Fix heavy download activity stalling listings and SMB transfers (13bb6fddb, 2d1558863)
- Fix the drive index failing and rebuilding on a nearly full disk (a9074a0b2, e85b92dd1)
- Fix the downloads hotkey landing on a download you had renamed or moved (8ade04435)
- Fix a drive holding only hidden items claiming macOS may be hiding its files (893dc78c4)
- Fix ⇧⌥← right after F2 doing nothing, and the third F2 not letting the extension grow into the name (4ae45b382,
  367b33e58)
- Fix Brief view's rename field stopping short of the column's edge (04253c47b, 367b33e58)
- Fix text selections in fields being nearly invisible with some accent colors (8085c04a2)
- Fix Copy no longer firing from the menu after rebinding it away from F5 and back (0c1c918f3)
- Fix the "Reset all keyboard shortcuts" confirmation showing in English with an OK button (52d5f7626)
- Fix a revoked or broken server certificate being met with "add it in Keychain Access" (a07ee595a)
- Fix a file name with a comma showing up unredacted in an error report after a failed move (999d82f2d)
- Fix expected refusals, like a full disk or a file in use, sending automatic error reports (7d9e63b8c, 242167bcd)
- Fix rare Android errors reading as a dropped connection (4cbe29171)
- Fix sizes and search drifting on Linux after a burst of file changes (5cc24bbe4)
- Fix Cmdr on Linux not finding adb in Android Studio's default SDK folder (b8429219c)
- Fix an empty gap before the viewer's title on Linux (174195fec)

### Security

- Stop AI tools from reading more mail, browser, and password manager data (Thunderbird, Outlook, Vivaldi, Opera,
  Bitwarden, 1Password, and Linux mail clients): CVSS v4.0 5.9 (medium), Cmdr 0.42.0–0.60.0 (99c77770a)

### Non-app

- Lay the groundwork for the Windows port: every OS difference sits behind a typed seam, copy and default keys vary per
  OS, and a Windows CI lane reports what still breaks (0076c104d, 757dcbf76, 092ad6a08, 2445e477a, 7d712987f, e9d234cec,
  0d36249bb, a5916d223)
- Move the repo, the backlog board, and the reports repo under the `getcmdr` org (2cb16896d, c67c95ca8)
- Turn the website's features page into a tour led by product clips, and say how to pronounce "Cmdr" on the home page
  (ee2764307, 352a02a92, 07f4e1155)
- Run the unattended agents as a restricted user, with David's approval required when they touch deploys, deps, or
  release code (20b54bd54, 74c65602e, 10a4e9f98, 738ae8aa9)
- Give translators tools to write, look up, and check a batch, with no hand-edited catalogs (1f1792935, fbc06c45c,
  a3ebb50ab)
- Show the acquisition funnel by country and channel in the analytics dashboard (daab3ec49, dab6c43bd, cc90658de)

## [0.60.0] - 2026-11-06

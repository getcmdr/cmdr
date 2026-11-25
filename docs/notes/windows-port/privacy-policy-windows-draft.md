# Privacy policy: the Windows crash-report sentence (draft)

**This must be in the published privacy policy before any Windows build is offered to anyone outside.** Windows isn't
shipped today, so the live policy is still true for every build anyone can get. The policy is legal copy David reviews
and publishes himself, and a `main` push touching `apps/website/` deploys the site, so the change waits here.

## What changed in the app (PR #1471, task 47 of #942)

A Windows crash report now carries, for both a native crash and a panic on a shipped build, the faulting thread's stack
as `<program file name>+0x<offset>` frames. Any module on that stack can appear by its file name: Cmdr itself, a part of
Windows (`ntdll.dll`), WebView2, and third-party software running inside Cmdr's process (antivirus hooks, shell
extensions, enterprise agents, screen readers). File names only, never the folders they're in. A native crash also
carries the one-line reason from Windows' exception record (like reading from a bad memory address, with that address).
Mechanism and evidence: `apps/desktop/src-tauri/src/crash_reporter/DETAILS.md` § Windows native crashes.

## The sentence it replaces

In `apps/website/src/pages/privacy-policy.astro`, the Crash reports item, today:

> On Windows, Cmdr reports only a crash its own code detects as it happens, and it doesn't read any crash file Windows
> writes. No filenames, no file contents.

## Proposed wording

> On Windows, we include, for each place the code stopped, the name of the program file it was in and how far into that
> file. That can be Cmdr itself, a part of Windows like `ntdll.dll`, or other software that runs inside Cmdr, such as an
> antivirus, a shell extension, a screen reader, or a tool your organization installed. It's the file's name only, never
> the folder it's in. For a crash with no message, we also include the one-line reason Windows gives (like reading from
> a bad memory address, and that address). Cmdr doesn't read any crash file Windows writes. No filenames of yours, no
> file contents.

package main

import (
	"fmt"
	"os"
	"strings"
	"time"
)

// pushBranch pushes HEAD to its feature branch unless origin already has it. It overwrites
// origin's branch only when this branch already integrated it (a rebase or squash of commits it
// had); origin commits it never had (another machine, GitHub's "Update branch") stop the land.
//
// It runs the pre-push hook itself first, with no connection open (`runPrePushHook`), and pushes
// with `--no-verify`. The hook may commit a formatting fix or regenerated notices and stop; then
// this runs it once more on that commit. It returns the SHA that's on origin, and whether it
// pushed it there.
func pushBranch(r runner, branch, head string) (string, bool, error) {
	remote, err := r.fetchBranch(branch)
	if err != nil {
		return "", false, err
	}
	if remote != "" && remote != head && !r.isAncestor(remote, head) && !r.inReflog(branch, remote) {
		return "", false, fmt.Errorf("%w (pushed from another machine, or by GitHub's \"Update branch\"), so I didn't overwrite them. Bring them in, then land again:\n  git pull --rebase origin %s\n  or take origin's version: git reset --hard origin/%s", errOriginAhead, branch, branch)
	}
	hookRetried, remoteRetries, hookPassed := false, 0, ""
	for remote != head {
		if hookPassed != head {
			if err := runPrePushHook(r, branch, head, remote); err != nil {
				newHead, headErr := r.git("rev-parse", "HEAD")
				if headErr != nil || newHead == head || hookRetried {
					return "", false, fmt.Errorf("the pre-push hook stopped the push; fix what it says, then land again: pnpm land\n%w", err)
				}
				say("The pre-push hook added a commit, so running it again on that.")
				head, hookRetried = newHead, true
				continue
			}
			hookPassed = head
		}
		say("Pushing %s...", branch)
		porcelain, err := pushToOrigin(r, branch)
		if err == nil {
			return head, true, nil
		}
		// Origin's side turned it away (GitHub's `fatal error in commit_refs`, 2026-11-06): the ref
		// didn't move, so pushing the same thing again is safe. A few short retries, then say so.
		if reason, rejected := remoteRejection(porcelain, "refs/heads/"+branch); rejected {
			next, wait := afterGHFailure(nil, transientRetries-remoteRetries, 0, time.Now())
			if next != ghRetryTransient {
				return "", false, &errRemoteRejected{reason: reason, cause: err}
			}
			remoteRetries++
			say("Origin turned the push away on its side (%s), so trying again in %s.", reason, wait.Round(time.Second))
			pause(wait + jitter(wait/10))
			continue
		}
		return "", false, fmt.Errorf("the branch push didn't go through; fix what it says, then land again: pnpm land\n%w", err)
	}
	return head, false, nil
}

// runPrePushHook runs the repo's pre-push hook the way `git push` would for this push, with the
// line git feeds it on stdin. Git runs the hook with the connection to origin already open, and a
// hook that takes a minute (regenerating the license notices) outlasts GitHub's idle timeout, so
// the push dies with `Read from remote host github.com: Operation timed out` (2026-11-18, three
// lands in a row). An error means the hook stopped the push.
func runPrePushHook(r runner, branch, head, remote string) error {
	if remote == "" {
		remote = strings.Repeat("0", len(head))
	}
	url, err := r.git("remote", "get-url", "origin")
	if err != nil {
		return err
	}
	stdin, err := os.CreateTemp("", "land-pre-push-*")
	if err != nil {
		return err
	}
	defer os.Remove(stdin.Name())
	_, err = fmt.Fprintf(stdin, "HEAD %s refs/heads/%s %s\n", head, branch, remote)
	if closeErr := stdin.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	out, err := r.cmd("", "git", "hook", "run", "--ignore-missing", "--to-stdin="+stdin.Name(), "pre-push", "--", "origin", url)
	if out = strings.TrimSpace(out); out != "" {
		say("%s", out)
	}
	return err
}

// errRemoteRejected: origin kept turning the branch push away on its side. On GitHub a feature
// branch has no rulesets, so that's GitHub failing, unless the reason names a rule of its own
// (secret scanning's push protection, a workflow file the token can't write).
type errRemoteRejected struct {
	reason string
	cause  error
}

func (e *errRemoteRejected) Error() string {
	return fmt.Sprintf("origin kept turning the branch push away on its side (%s), so nothing changed. When the reason is GitHub's own (like \"failure\"), GitHub is having trouble: run pnpm land again in a few minutes. When it names a rule, fix that first.\n%v", e.reason, e.cause)
}

func (e *errRemoteRejected) Unwrap() error { return e.cause }

// pushToOrigin pushes HEAD to origin's branch and returns git's `--porcelain` report. The bare
// lease checks origin against the remote-tracking ref `fetchBranch` just updated, and
// `--force-if-includes` refuses when that ref's tip isn't in the local branch's history: the
// backstop for a push that lands between `fetchBranch` and here. (An explicit lease value would
// make git ignore `--force-if-includes`.) `--no-verify`: `pushBranch` already ran the hook on HEAD.
func pushToOrigin(r runner, branch string) (string, error) {
	return r.git("push", "--porcelain", "--no-verify", "--force-with-lease=refs/heads/"+branch, "--force-if-includes", "origin", "HEAD:refs/heads/"+branch)
}

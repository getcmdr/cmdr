package checks

import (
	"fmt"
	"math/rand/v2"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
)

// RunWebsiteE2E runs the website's Playwright suite: every spec in the `chromium` project, which
// holds only the browser demo's smoke. The website lane runs it against the RELEASED demo image,
// which can predate `main`'s frontend (`apps/website/e2e/demo-support.ts`).
func RunWebsiteE2E(ctx *CheckContext) (CheckResult, error) {
	return runWebsitePlaywright(ctx, []string{"chromium"}, false)
}

// RunWebsiteDemoE2E runs the full browser demo suite (the smoke plus `demo.spec.ts`) against the
// site's `dist/demo/`, which must exist: the release pipeline's `demo` job and the nightly CI run
// build the demo from the same sources first. Three projects, one per visitor OS: the demo answers a
// Windows visitor (`demo`), a Mac one (`demo-mac`), and a Linux one (`demo-linux`) with their own
// OS's keys, copy, and window chrome.
func RunWebsiteDemoE2E(ctx *CheckContext) (CheckResult, error) {
	return runWebsitePlaywright(ctx, []string{"demo", "demo-mac", "demo-linux"}, true)
}

func runWebsitePlaywright(ctx *CheckContext, projects []string, needsDemo bool) (CheckResult, error) {
	websiteDir := filepath.Join(ctx.RootDir, "apps", "website")

	// Without a built site, `serve` answers 404 on every path, which Playwright never counts as ready:
	// the run sits out the full `webServer` timeout and then reports a timeout that doesn't mention the
	// build. Naming this check alone doesn't run its `website-build` dependency, so that's reachable.
	if _, err := os.Stat(filepath.Join(websiteDir, "dist", "index.html")); os.IsNotExist(err) {
		return CheckResult{}, fmt.Errorf("apps/website/dist/index.html not found: build the site first (pnpm check website-build website-e2e)")
	}
	// The demo suite skips itself without a demo off CI, which would read as a pass here.
	if needsDemo {
		if _, err := os.Stat(filepath.Join(websiteDir, "dist", "demo", "index.html")); os.IsNotExist(err) {
			return CheckResult{}, fmt.Errorf("apps/website/dist/demo/index.html not found: run `pnpm build:demo` at the repo root, then `CMDR_WEBSITE_DEMO=1 pnpm build` in apps/website")
		}
	}

	port, err := pickWebsiteE2EPort()
	if err != nil {
		return CheckResult{}, err
	}

	args := []string{"exec", "playwright", "test", "--reporter=list"}
	for _, project := range projects {
		args = append(args, "--project="+project)
	}
	cmd := exec.Command("pnpm", args...)
	cmd.Dir = websiteDir
	cmd.Env = append(os.Environ(), websiteE2EPortEnv+"="+strconv.Itoa(port))
	output, err := RunCommand(cmd, true)
	if err != nil {
		return CheckResult{}, fmt.Errorf("e2e tests failed\n%s", indentOutput(output))
	}

	// Extract test count
	re := regexp.MustCompile(`(\d+) passed`)
	matches := re.FindStringSubmatch(output)
	if len(matches) > 1 {
		count, _ := strconv.Atoi(matches[1])
		return Success(fmt.Sprintf("%d %s passed", count, Pluralize(count, "test", "tests"))), nil
	}
	return Success("All E2E tests passed"), nil
}

// websiteE2EPortEnv carries the run's port to `apps/website/playwright.config.ts`, which serves
// `dist` on it (bound to 127.0.0.1) and points `baseURL` there. Each run gets its own, so two
// worktrees running the website suites at once don't collide on one fixed port (they did on
// 18473, 2026-11-19).
const websiteE2EPortEnv = "CMDR_WEBSITE_E2E_PORT"

// The project's range for dev services' ports.
const (
	websiteE2EPortMin = 10000
	websiteE2EPortMax = 29999
)

// pickWebsiteE2EPort picks a random port in the project's range that 127.0.0.1 can bind right
// now. The probe closes before `serve` binds, so another process could still take it in between;
// `reuseExistingServer: false` makes that a clear startup failure naming the port, never a run
// against the wrong server.
func pickWebsiteE2EPort() (int, error) {
	for range 50 {
		port := websiteE2EPortMin + rand.IntN(websiteE2EPortMax-websiteE2EPortMin+1)
		l, err := net.Listen("tcp", net.JoinHostPort("127.0.0.1", strconv.Itoa(port)))
		if err != nil {
			continue
		}
		_ = l.Close()
		return port, nil
	}
	return 0, fmt.Errorf("couldn't find a free port in %d–%d on 127.0.0.1 for the website's Playwright server",
		websiteE2EPortMin, websiteE2EPortMax)
}

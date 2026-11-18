package checks

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strings"
)

// The website image runs `pnpm install` on a hand-picked slice of the repo (`.dockerignore`
// lets files in, `apps/website/Dockerfile` copies them), and pnpm refuses the install when any
// file the workspace config names is missing, even under a `--filter` that never installs it
// (`ERR_PNPM_PATCH_NOT_FOUND` for a `patchedDependencies` entry). The full image build
// (`website-docker-build`) is a Slow checks job, so this scanner gives the landing CI the same
// signal in milliseconds: every root file the install reads must pass `.dockerignore` and be
// copied before the `RUN pnpm install` step.
const (
	dockerInstallWorkspaceFile = "pnpm-workspace.yaml"
	dockerInstallIgnoreFile    = ".dockerignore"
	dockerInstallDockerfile    = "apps/website/Dockerfile"
)

// dockerInstallRootFiles are what `pnpm install` always reads from the workspace root.
var dockerInstallRootFiles = []string{"package.json", dockerInstallWorkspaceFile, "pnpm-lock.yaml"}

// websiteDockerInstallContextInputs are the three files the check reads. The patch files
// themselves don't matter, only the paths the workspace config names.
var websiteDockerInstallContextInputs = []string{dockerInstallWorkspaceFile, dockerInstallIgnoreFile, dockerInstallDockerfile}

func RunWebsiteDockerInstallContext(ctx *CheckContext) (CheckResult, error) {
	var contents [3]string
	for i, rel := range websiteDockerInstallContextInputs {
		raw, err := os.ReadFile(filepath.Join(ctx.RootDir, rel))
		if err != nil {
			return CheckResult{}, fmt.Errorf("read %s: %w", rel, err)
		}
		contents[i] = string(raw)
	}
	problems := dockerInstallContextProblems(contents[0], contents[1], contents[2])
	if len(problems) > 0 {
		return CheckResult{}, fmt.Errorf("the website image's `pnpm install` can't see every file it needs\n%s",
			indentOutput(strings.Join(problems, "\n")+
				"\nlet each through "+dockerInstallIgnoreFile+" and COPY it in "+dockerInstallDockerfile+" before `RUN pnpm install`"))
	}
	needed := len(dockerInstallRootFiles) + len(pnpmWorkspacePatchFiles(contents[0]))
	return Success(fmt.Sprintf("all %d files `pnpm install` reads reach the website image before the install", needed)), nil
}

// dockerInstallContextProblems returns one line per needed file that `.dockerignore` keeps out
// or the Dockerfile doesn't copy before its first `pnpm install`.
func dockerInstallContextProblems(workspace, dockerignore, dockerfile string) []string {
	copied, found := dockerfileSourcesBeforeInstall(dockerfile)
	if !found {
		return []string{dockerInstallDockerfile + " has no `RUN pnpm install` step to check"}
	}
	var problems []string
	for _, file := range append(append([]string{}, dockerInstallRootFiles...), pnpmWorkspacePatchFiles(workspace)...) {
		if !dockerIgnoreIncludes(dockerignore, file) {
			problems = append(problems, fmt.Sprintf("%s: excluded by %s", file, dockerInstallIgnoreFile))
		}
		if !copiedBy(copied, file) {
			problems = append(problems, fmt.Sprintf("%s: not copied before `pnpm install` in %s", file, dockerInstallDockerfile))
		}
	}
	return problems
}

// pnpmWorkspacePatchFiles returns the patch file paths under `patchedDependencies:`, read line by
// line (the check module carries no YAML parser, and the block is a flat `name: path` map).
func pnpmWorkspacePatchFiles(workspace string) []string {
	var files []string
	inBlock := false
	for _, line := range strings.Split(workspace, "\n") {
		trimmed := strings.TrimSpace(line)
		if !inBlock {
			inBlock = trimmed == "patchedDependencies:"
			continue
		}
		if trimmed == "" || strings.HasPrefix(trimmed, "#") {
			continue
		}
		if !strings.HasPrefix(line, " ") && !strings.HasPrefix(line, "\t") {
			break // the next top-level key
		}
		// The key is often quoted (`'@scope/pkg@1.0.0'`); the path follows the last `: `.
		idx := strings.LastIndex(trimmed, ": ")
		if idx < 0 {
			continue
		}
		files = append(files, strings.Trim(strings.TrimSpace(trimmed[idx+2:]), `'"`))
	}
	return files
}

// dockerfileSourcesBeforeInstall returns the sources of every build-context `COPY` (not
// `--from`) before the first `RUN pnpm install`, and whether that step exists.
func dockerfileSourcesBeforeInstall(dockerfile string) ([]string, bool) {
	var sources []string
	for _, line := range strings.Split(dockerfile, "\n") {
		fields := strings.Fields(line)
		if len(fields) == 0 {
			continue
		}
		switch strings.ToUpper(fields[0]) {
		case "RUN":
			if strings.Contains(line, "pnpm install") {
				return sources, true
			}
		case "COPY":
			args := fields[1:]
			fromStage := false
			for len(args) > 0 && strings.HasPrefix(args[0], "--") {
				fromStage = fromStage || strings.HasPrefix(args[0], "--from")
				args = args[1:]
			}
			if !fromStage && len(args) >= 2 {
				sources = append(sources, args[:len(args)-1]...)
			}
		}
	}
	return sources, false
}

// copiedBy reports whether a COPY source is the file itself or a dir holding it.
func copiedBy(sources []string, file string) bool {
	for _, src := range sources {
		src = strings.TrimPrefix(src, "./")
		if src == "." || src == file || strings.HasPrefix(file, strings.TrimSuffix(src, "/")+"/") {
			return true
		}
	}
	return false
}

// dockerIgnoreIncludes reports whether a root-relative path reaches the build context: the last
// rule that matches it (a glob on the path or a parent dir of it) decides, `!` letting it in.
func dockerIgnoreIncludes(dockerignore, file string) bool {
	included := true
	for _, line := range strings.Split(dockerignore, "\n") {
		rule := strings.TrimSpace(line)
		if rule == "" || strings.HasPrefix(rule, "#") {
			continue
		}
		negated := strings.HasPrefix(rule, "!")
		pattern := strings.TrimSuffix(strings.TrimPrefix(strings.TrimPrefix(rule, "!"), "/"), "/")
		if dockerIgnoreMatches(pattern, file) {
			included = negated
		}
	}
	return included
}

func dockerIgnoreMatches(pattern, file string) bool {
	for p := file; p != "." && p != "/" && p != ""; p = path.Dir(p) {
		if ok, _ := path.Match(pattern, p); ok {
			return true
		}
	}
	return false
}

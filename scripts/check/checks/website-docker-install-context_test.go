package checks

import (
	"reflect"
	"testing"
)

const dockerInstallWorkspaceFixture = `packages:
  - apps/*

patchedDependencies:
  '@scope/pkg@1.0.0': patches/@scope__pkg@1.0.0.patch
  other@2.0.0: "patches/other@2.0.0.patch"

onlyBuiltDependencies:
  - esbuild
`

const dockerInstallIgnoreGood = `# comment
*
!package.json
!pnpm-workspace.yaml
!pnpm-lock.yaml
!patches/
!apps/website/
apps/website/node_modules
`

const dockerInstallDockerfileGood = `FROM node AS builder
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY patches/ ./patches/
COPY apps/website/package.json ./apps/website/
RUN pnpm install --frozen-lockfile --filter "./apps/website"
COPY apps/website/ ./apps/website/
`

func TestDockerInstallPatchFiles(t *testing.T) {
	got := pnpmWorkspacePatchFiles(dockerInstallWorkspaceFixture)
	want := []string{"patches/@scope__pkg@1.0.0.patch", "patches/other@2.0.0.patch"}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("patch files = %v, want %v", got, want)
	}
}

func TestDockerInstallContextPasses(t *testing.T) {
	problems := dockerInstallContextProblems(dockerInstallWorkspaceFixture, dockerInstallIgnoreGood, dockerInstallDockerfileGood)
	if len(problems) != 0 {
		t.Errorf("problems = %v, want none", problems)
	}
}

// Regression anchor: `c50c9531ec` added a patch while the image copied no `patches/`, and pnpm
// refused the install (`ERR_PNPM_PATCH_NOT_FOUND`) only in the nightly Docker build.
func TestDockerInstallContextFlagsAPatchTheImageNeverSees(t *testing.T) {
	ignore := `*
!package.json
!pnpm-workspace.yaml
!pnpm-lock.yaml
!apps/website/
`
	dockerfile := `FROM node
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile --filter "./apps/website"
COPY patches/ ./patches/
`
	problems := dockerInstallContextProblems(dockerInstallWorkspaceFixture, ignore, dockerfile)
	// Each patch file: excluded by `.dockerignore`, and copied only after the install.
	if len(problems) != 4 {
		t.Errorf("problems = %v, want four (two per patch file)", problems)
	}
}

func TestDockerInstallContextFlagsAMissingInstallStep(t *testing.T) {
	problems := dockerInstallContextProblems(dockerInstallWorkspaceFixture, dockerInstallIgnoreGood, "FROM node\nCOPY . .\n")
	if len(problems) != 1 {
		t.Errorf("problems = %v, want one about the missing `pnpm install`", problems)
	}
}

func TestDockerIgnoreIncludes(t *testing.T) {
	cases := []struct {
		path string
		want bool
	}{
		{"patches/x.patch", true},
		{"package.json", true},
		{"apps/website/src/a.ts", true},
		{"apps/website/node_modules/a/b.js", false},
		{"CHANGELOG.md", false},
	}
	for _, c := range cases {
		if got := dockerIgnoreIncludes(dockerInstallIgnoreGood, c.path); got != c.want {
			t.Errorf("dockerIgnoreIncludes(%q) = %v, want %v", c.path, got, c.want)
		}
	}
}

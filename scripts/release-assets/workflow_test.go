package main

import (
	"fmt"
	"regexp"
	"slices"
	"strings"
)

// A reader for the subset of GitHub Actions YAML `release-pipeline.yml` uses, enough for the
// pipeline dry run to pull each job's steps out and run them. Line-based on purpose: the
// workflow's layout is regular (two-space indents, block scalars for scripts), and a YAML library
// would be the module's only dependency. Anything it doesn't understand fails the test.

type wfStep struct {
	Name string
	ID   string
	If   string
	Uses string
	Run  string
	Env  map[string]string
	With map[string]string
}

type wfJob struct {
	ID    string
	Steps []wfStep
}

var (
	jobLine     = regexp.MustCompile(`^  ([a-z0-9-]+):\s*$`)
	stepsLine   = regexp.MustCompile(`^    steps:\s*$`)
	stepStart   = regexp.MustCompile(`^      - ([a-z-]+):\s*(.*)$`)
	stepKeyLine = regexp.MustCompile(`^        ([a-z-]+):\s*(.*)$`)
	mapKeyLine  = regexp.MustCompile(`^          ([A-Za-z0-9_-]+):\s*(.*)$`)
	trailingCmt = regexp.MustCompile(`\s+#.*$`)
)

func indentOf(line string) int { return len(line) - len(strings.TrimLeft(line, " ")) }

// blockScalar collects the lines of a `|` block that starts after lines[i], all indented deeper
// than `parent`, and returns the text (with its trailing newline) and the index of the last line.
func blockScalar(lines []string, i, parent int) (string, int) {
	var body []string
	strip := -1
	j := i + 1
	for ; j < len(lines); j++ {
		l := lines[j]
		if strings.TrimSpace(l) == "" {
			body = append(body, "")
			continue
		}
		if indentOf(l) <= parent {
			break
		}
		if strip < 0 {
			strip = indentOf(l)
		}
		body = append(body, l[min(strip, indentOf(l)):])
	}
	for len(body) > 0 && body[len(body)-1] == "" {
		body = body[:len(body)-1]
	}
	return strings.Join(body, "\n") + "\n", j - 1
}

// plainContinuation joins a plain scalar's continuation lines (deeper than `parent`) with spaces.
func plainContinuation(lines []string, i, parent int) (string, int) {
	var parts []string
	j := i + 1
	for ; j < len(lines); j++ {
		l := lines[j]
		if strings.TrimSpace(l) == "" || indentOf(l) <= parent {
			break
		}
		parts = append(parts, strings.TrimSpace(l))
	}
	return strings.Join(parts, " "), j - 1
}

func unquote(v string) string {
	switch {
	case len(v) >= 2 && v[0] == '\'' && v[len(v)-1] == '\'':
		return strings.ReplaceAll(v[1:len(v)-1], "''", "'")
	case len(v) >= 2 && v[0] == '"' && v[len(v)-1] == '"':
		return v[1 : len(v)-1]
	default:
		return trailingCmt.ReplaceAllString(v, "")
	}
}

// scalar reads the value that starts on lines[i] (inline `v` after the key), which may be a block
// or continue on deeper lines.
func scalar(lines []string, i, parent int, v string) (string, int) {
	switch v {
	case "|", "|-":
		return blockScalar(lines, i, parent)
	case ">-", ">":
		s, end := plainContinuation(lines, i, parent)
		return s, end
	case "":
		return plainContinuation(lines, i, parent)
	}
	return unquote(v), i
}

func parseMap(lines []string, i int) (map[string]string, int) {
	m := map[string]string{}
	j := i + 1
	for ; j < len(lines); j++ {
		l := lines[j]
		if strings.TrimSpace(l) == "" || strings.HasPrefix(strings.TrimSpace(l), "#") {
			continue
		}
		if indentOf(l) <= 8 {
			break
		}
		mm := mapKeyLine.FindStringSubmatch(l)
		if mm == nil {
			continue
		}
		v, end := scalar(lines, j, 10, mm[2])
		m[mm[1]] = v
		j = end
	}
	return m, j - 1
}

func (s *wfStep) set(key, value string) error {
	switch key {
	case "name":
		s.Name = value
	case "id":
		s.ID = value
	case "if":
		s.If = value
	case "uses":
		s.Uses = value
	case "run":
		s.Run = value
	case "working-directory", "continue-on-error", "shell", "timeout-minutes":
		// Not used by the jobs the dry run runs.
	default:
		return fmt.Errorf("step %q: unknown key %q", s.Name, key)
	}
	return nil
}

type wfParser struct {
	lines   []string
	jobs    map[string]*wfJob
	job     *wfJob
	step    *wfStep
	inSteps bool
}

func (p *wfParser) flush() {
	if p.step != nil && p.job != nil {
		p.job.Steps = append(p.job.Steps, *p.step)
	}
	p.step = nil
}

// stepLine reads the step-list line at i and returns the index of the last line it consumed.
func (p *wfParser) stepLine(i int) (int, error) {
	l := p.lines[i]
	if strings.TrimSpace(l) != "" && indentOf(l) <= 4 && !strings.HasPrefix(strings.TrimSpace(l), "#") {
		p.flush()
		p.inSteps = false
		return i, nil
	}
	if m := stepStart.FindStringSubmatch(l); m != nil {
		p.flush()
		p.step = &wfStep{}
		v, end := scalar(p.lines, i, 8, m[2])
		return end, p.step.set(m[1], v)
	}
	m := stepKeyLine.FindStringSubmatch(l)
	if m == nil || p.step == nil {
		return i, nil
	}
	switch m[1] {
	case "env":
		env, end := parseMap(p.lines, i)
		p.step.Env = env
		return end, nil
	case "with":
		with, end := parseMap(p.lines, i)
		p.step.With = with
		return end, nil
	}
	v, end := scalar(p.lines, i, 8, m[2])
	return end, p.step.set(m[1], v)
}

// parseWorkflow returns the workflow's jobs by id, each with its steps in order.
func parseWorkflow(src string) (map[string]*wfJob, error) {
	p := &wfParser{lines: strings.Split(src, "\n"), jobs: map[string]*wfJob{}}
	start := slices.Index(p.lines, "jobs:")
	if start < 0 {
		return nil, fmt.Errorf("the workflow has no jobs")
	}
	for i := start + 1; i < len(p.lines); i++ {
		l := p.lines[i]
		switch m := jobLine.FindStringSubmatch(l); {
		case m != nil:
			p.flush()
			p.job = &wfJob{ID: m[1]}
			p.jobs[m[1]] = p.job
			p.inSteps = false
		case stepsLine.MatchString(l):
			p.flush()
			p.inSteps = true
		case p.inSteps && p.job != nil:
			end, err := p.stepLine(i)
			if err != nil {
				return nil, err
			}
			i = end
		}
	}
	p.flush()
	return p.jobs, nil
}

var exprRE = regexp.MustCompile(`\$\{\{\s*(.*?)\s*\}\}`)

// expand replaces every `${{ … }}` in s through resolve, failing on the first it can't answer.
func expand(s string, resolve func(string) (string, error)) (string, error) {
	var firstErr error
	out := exprRE.ReplaceAllStringFunc(s, func(m string) string {
		v, err := resolve(exprRE.FindStringSubmatch(m)[1])
		if err != nil && firstErr == nil {
			firstErr = err
		}
		return v
	})
	return out, firstErr
}

var condTerm = regexp.MustCompile(`^(\S+)\s*(==|!=)\s*'([^']*)'$`)

// evalIf answers the step-level `if:` shapes the release jobs use: `a == 'b'` and `a != 'b'`
// terms joined by `&&`, with or without the `${{ }}` wrapper.
func evalIf(cond string, resolve func(string) (string, error)) (bool, error) {
	cond = strings.TrimSpace(cond)
	if cond == "" {
		return true, nil
	}
	if m := exprRE.FindStringSubmatch(cond); m != nil && m[0] == cond {
		cond = m[1]
	}
	for term := range strings.SplitSeq(cond, "&&") {
		m := condTerm.FindStringSubmatch(strings.TrimSpace(term))
		if m == nil {
			return false, fmt.Errorf("the dry run can't evaluate `if: %s`; teach evalIf", cond)
		}
		v, err := resolve(m[1])
		if err != nil {
			return false, err
		}
		if (v == m[3]) != (m[2] == "==") {
			return false, nil
		}
	}
	return true, nil
}

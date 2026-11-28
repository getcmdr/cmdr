package main

import (
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"strings"
	"time"
)

// Phone pings for the moments a release needs David or is worth coming back for: the signing jobs
// waiting for his approval, the release out, and the run stopping. They go to the private ntfy
// server's `alerts-cmdr` topic, which his phone subscribes to, through the write-only
// `cmdr-release` account (infra repo `docs/ntfy.md`).
//
// A ping is best effort and never stops or fails a release: no token in sops, a dry run, or an
// unreachable server only prints one line and carries on.

const (
	ntfyTopicURL    = "https://ntfy.veszelovszki.com/alerts-cmdr"
	ntfyTokenSecret = "NTFY_CMDR_RELEASE_TOKEN" // the sops name `secret` resolves
	// CMDR_RELEASE_NTFY=off silences every ping, for a run David is watching anyway.
	ntfyOffEnv = "CMDR_RELEASE_NTFY"
)

// ping is one message: `Priority` decides how loudly the phone rings (`min`, `low`, `default`,
// `high`, `urgent`), and `Tags` takes emoji short codes the app shows as an icon.
type ping struct {
	key      string // sends once per key per run
	title    string
	body     string
	priority string
	tags     string
	link     string
}

type notifier struct {
	tag    string // the release's tag, for the stop ping `fail` sends
	url    string
	token  string
	client *http.Client
	sent   map[string]bool
}

// alerts is the run's notifier: a real release sets it in `dispatch`, and nil (every other mode)
// sends nothing.
var alerts *notifier

// newNotifier reads the token once. An empty token makes a notifier that only says it's silent.
func newNotifier(readToken func() (string, error)) *notifier {
	n := &notifier{url: ntfyTopicURL, client: &http.Client{Timeout: 10 * time.Second}, sent: map[string]bool{}}
	if os.Getenv(ntfyOffEnv) == "off" {
		fmt.Printf("    (phone pings off: %s=off)\n", ntfyOffEnv)
		return n
	}
	token, err := readToken()
	if err != nil || token == "" {
		fmt.Printf("    (no phone pings: `secret %s` didn't answer)\n", ntfyTokenSecret)
		return n
	}
	n.token = token
	return n
}

func secretToken() (string, error) {
	out, err := exec.Command("secret", ntfyTokenSecret).Output()
	return strings.TrimSpace(string(out)), err
}

func (n *notifier) send(p ping) {
	if n == nil || n.token == "" || n.sent[p.key] {
		return
	}
	n.sent[p.key] = true
	req, err := http.NewRequest(http.MethodPost, n.url, strings.NewReader(p.body))
	if err != nil {
		return
	}
	req.Header.Set("Authorization", "Bearer "+n.token)
	req.Header.Set("Title", p.title)
	req.Header.Set("Priority", p.priority)
	if p.tags != "" {
		req.Header.Set("Tags", p.tags)
	}
	if p.link != "" {
		req.Header.Set("Click", p.link)
	}
	resp, err := n.client.Do(req)
	if err != nil || resp == nil {
		fmt.Printf("    (phone ping didn't go out: %v)\n", err)
		return
	}
	_ = resp.Body.Close()
	if resp.StatusCode/100 != 2 {
		fmt.Printf("    (phone ping didn't go out: ntfy answered %s)\n", resp.Status)
	}
}

// awaitingApproval: a job GitHub holds at `waiting`, which in this repo is only `sign` behind the
// `release` environment's required reviewer (`ciJob.state`).
func awaitingApproval(jobs []ciJob) bool {
	for _, j := range jobs {
		if j.Status == "waiting" {
			return true
		}
	}
	return false
}

func approvalPing(tag string, runLink string) ping {
	return ping{
		key:      "approval",
		title:    fmt.Sprintf("Cmdr %s needs your signing approval", tag),
		body:     "The build is done. Open the run, then Review deployments and approve `release`. One approval covers all three signing jobs.",
		priority: "high",
		tags:     "key",
		link:     runLink,
	}
}

func outPing(tag string, runLink string) ping {
	return ping{
		key:      "out",
		title:    fmt.Sprintf("Cmdr %s is out", tag),
		body:     "Published, signed, and on the update feed.",
		priority: "default",
		tags:     "white_check_mark",
		link:     runLink,
	}
}

func stoppedPing(tag string, err error) ping {
	return ping{
		key:      "stopped",
		title:    fmt.Sprintf("Cmdr %s release stopped", tag),
		body:     fmt.Sprintf("%v\n\nFix the cause and re-run release-finish.sh: it picks up where it stopped.", err),
		priority: "high",
		tags:     "warning",
	}
}

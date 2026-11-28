package main

import (
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

type seenPing struct {
	auth, title, priority, click, body string
}

func ntfyDouble(t *testing.T) (*httptest.Server, *[]seenPing) {
	t.Helper()
	var got []seenPing
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		got = append(got, seenPing{
			auth:     r.Header.Get("Authorization"),
			title:    r.Header.Get("Title"),
			priority: r.Header.Get("Priority"),
			click:    r.Header.Get("Click"),
			body:     string(body),
		})
	}))
	t.Cleanup(srv.Close)
	return srv, &got
}

func TestAPingGoesOutOncePerKeyWithTheTokenAndPriority(t *testing.T) {
	srv, got := ntfyDouble(t)
	n := newNotifier(func() (string, error) { return "tk_test", nil })
	n.url = srv.URL

	link := "https://github.com/getcmdr/cmdr/actions/runs/1"
	n.send(approvalPing("v1.2.3", link))
	n.send(approvalPing("v1.2.3", link)) // the wait loop sees `waiting` on every poll
	n.send(outPing("v1.2.3", link))

	if len(*got) != 2 {
		t.Fatalf("want one approval ping and one out ping, got %d: %+v", len(*got), *got)
	}
	first := (*got)[0]
	if first.auth != "Bearer tk_test" || first.priority != "high" || first.click != link {
		t.Errorf("approval ping headers: %+v", first)
	}
	if !strings.Contains(first.title, "v1.2.3") || !strings.Contains(first.title, "approval") {
		t.Errorf("approval ping title: %q", first.title)
	}
	if (*got)[1].priority != "default" {
		t.Errorf("out ping priority: %q", (*got)[1].priority)
	}
}

func TestNoTokenMeansNoPingAndNoFailure(t *testing.T) {
	srv, got := ntfyDouble(t)
	n := newNotifier(func() (string, error) { return "", errors.New("no such secret") })
	n.url = srv.URL
	n.send(stoppedPing("v1.2.3", errors.New("CI went red")))
	if len(*got) != 0 {
		t.Fatalf("a notifier without a token sent %d pings", len(*got))
	}
}

func TestPingsCanBeTurnedOff(t *testing.T) {
	t.Setenv(ntfyOffEnv, "off")
	srv, got := ntfyDouble(t)
	n := newNotifier(func() (string, error) { return "tk_test", nil })
	n.url = srv.URL
	n.send(outPing("v1.2.3", ""))
	if len(*got) != 0 {
		t.Fatalf("%s=off still sent %d pings", ntfyOffEnv, len(*got))
	}
}

func TestANilNotifierSendsNothing(t *testing.T) {
	var n *notifier // a dry run and the -ci-gate mode never set one
	n.send(outPing("v1.2.3", ""))
}

func TestAnUnreachableServerDoesntFailTheRelease(t *testing.T) {
	n := newNotifier(func() (string, error) { return "tk_test", nil })
	n.url = "http://127.0.0.1:1" // nothing listens there
	n.send(outPing("v1.2.3", ""))
}

func TestOnlyAWaitingJobReadsAsAwaitingApproval(t *testing.T) {
	jobs := []ciJob{
		{Name: "Release / Compile (aarch64)", Status: "completed", Conclusion: "success"},
		{Name: "Release / Sign and notarize (aarch64)", Status: "queued"},
	}
	if awaitingApproval(jobs) {
		t.Fatal("a queued job isn't waiting for approval")
	}
	jobs[1].Status = "waiting"
	if !awaitingApproval(jobs) {
		t.Fatal("a waiting job is waiting for approval")
	}
}

func TestTheStopPingSaysWhatStoppedIt(t *testing.T) {
	p := stoppedPing("v1.2.3", errors.New("the CI job `Rust` went red"))
	if p.priority != "high" || !strings.Contains(p.body, "went red") || !strings.Contains(p.body, "re-run") {
		t.Errorf("stop ping: %+v", p)
	}
}

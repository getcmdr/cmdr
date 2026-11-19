package checks

import (
	"net"
	"strconv"
	"testing"
)

// Two worktrees running the website suites at once collided on a fixed port; each run now
// picks its own in the project's range, and it must be one 127.0.0.1 can bind.
func TestPickWebsiteE2EPortIsFreeAndInRange(t *testing.T) {
	seen := map[int]bool{}
	for range 20 {
		port, err := pickWebsiteE2EPort()
		if err != nil {
			t.Fatal(err)
		}
		if port < websiteE2EPortMin || port > websiteE2EPortMax {
			t.Fatalf("port %d is outside %d–%d", port, websiteE2EPortMin, websiteE2EPortMax)
		}
		l, err := net.Listen("tcp", net.JoinHostPort("127.0.0.1", strconv.Itoa(port)))
		if err != nil {
			t.Fatalf("the picked port %d isn't bindable: %v", port, err)
		}
		_ = l.Close()
		seen[port] = true
	}
	if len(seen) < 2 {
		t.Fatalf("20 picks gave one port (%v); the pick isn't random", seen)
	}
}

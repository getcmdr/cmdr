package main

import (
	"os"
	"reflect"
	"regexp"
	"sort"
	"strconv"
	"testing"
)

const testMain = "refs/heads/main"

func cacheAt(id int64, ref, key, accessed, created string) cacheEntry {
	return cacheEntry{id: id, ref: ref, key: key, lastAccessed: accessed, created: created}
}

func prunedIDs(vs []pruneVerdict) []int64 {
	ids := []int64{}
	for _, v := range vs {
		ids = append(ids, v.entry.id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	return ids
}

func TestCacheFamily(t *testing.T) {
	cases := map[string]string{
		"v0-rust-desktop-rust-Linux-x64-2fe1f60a-f6f92938":                          "v0-rust-desktop-rust-Linux-x64",
		"v0-rust-desktop-rust-windows-tests-Windows_NT-x64-00bddf63-f6f92938":       "v0-rust-desktop-rust-windows-tests-Windows_NT-x64",
		"Linux-docker-e2e-59c3294656f52448ca65bfc9699464f9be8a5a9e1dbfa68804e9b1b3": "Linux-docker-e2e",
		"light-go-Linux-X64-6c39ecdaee8d7f20a2f714fa180c03c8":                       "light-go-Linux-X64",
		"pnpm-store-Linux-X64-container-46ac8157a1b243f06e702bddc880ccae":           "pnpm-store-Linux-X64-container",
		"wasm-toolchain-Linux-X64-4854c2aae1a89265e918b7c5a3bfb060":                 "wasm-toolchain-Linux-X64",
		// mise: only the config hash goes; the `install_args` hash before it is a variant.
		"mise-v1-linux-x64-ubuntu26-32bd160f9b67579ff18f6d172d5a826f":          "mise-v1-linux-x64-ubuntu26",
		"mise-v1-linux-x64-ubuntu26-4cd0e21a9a0795a14ec9-32bd160f9b67579ff18f": "mise-v1-linux-x64-ubuntu26-4cd0e21a9a0795a14ec9",
		// Left alone: markers (each its own family's only member), and caches nothing here owns.
		"lane-green-website-61ef23062e7458ed32bf8be1d2383dd3647ba5b79b6ca904":    "",
		"lane-markers-bin-Linux-X64-3f17d7e1692dc5d77275bcbb205d40dfc059e584":    "",
		"codeql-trap-1-2.27.5-javascript-5b1c2b99ea6578915b671659739333953f0bf4": "",
		"tauri-apt-ubuntu26-20261116.202.1-ca133994ac8ba062c9be6839264d6d4de4a9": "",
	}
	for key, want := range cases {
		if got := cacheFamily(key); got != want {
			t.Errorf("cacheFamily(%q) = %q, want %q", key, got, want)
		}
	}
}

func TestPlanPruneKeepsTheMostRecentlyUsedAndTheNewestOfEachMainFamily(t *testing.T) {
	caches := []cacheEntry{
		// A rotation: the new generation is both newest and most recently used, so the old goes.
		cacheAt(1, testMain, "v0-rust-desktop-rust-Linux-x64-aaaaaaaa-11111111", "2026-11-23T08:00:00Z", "2026-11-20T08:00:00Z"),
		cacheAt(2, testMain, "v0-rust-desktop-rust-Linux-x64-bbbbbbbb-11111111", "2026-11-23T09:00:00Z", "2026-11-23T09:00:00Z"),
		// An older-based PR restored the old generation after the new one's save: both stay.
		cacheAt(3, testMain, "Linux-docker-e2e-cccccccccccccccc", "2026-11-23T10:00:00Z", "2026-11-20T08:00:00Z"),
		cacheAt(4, testMain, "Linux-docker-e2e-dddddddddddddddd", "2026-11-23T09:00:00Z", "2026-11-23T09:00:00Z"),
		// Three generations: the oldest, neither newest nor most used, goes.
		cacheAt(5, testMain, "wasm-toolchain-Linux-X64-eeeeeeeeeeee", "2026-11-18T08:00:00Z", "2026-11-17T08:00:00Z"),
		cacheAt(6, testMain, "wasm-toolchain-Linux-X64-ffffffffffff", "2026-11-23T09:00:00Z", "2026-11-22T08:00:00Z"),
		cacheAt(7, testMain, "wasm-toolchain-Linux-X64-999999999999", "2026-11-22T09:00:00Z", "2026-11-23T08:00:00Z"),
		// A family of one stays.
		cacheAt(8, testMain, "light-go-Linux-X64-123456789abc", "2026-11-01T08:00:00Z", "2026-11-01T08:00:00Z"),
		// Markers on `main` never go, however old.
		cacheAt(9, testMain, "lane-green-website-aaaaaaaaaaaa", "2026-11-17T08:00:00Z", "2026-11-17T08:00:00Z"),
		cacheAt(10, testMain, "lane-green-website-bbbbbbbbbbbb", "2026-11-23T08:00:00Z", "2026-11-23T08:00:00Z"),
		// Another branch's scope is someone else's business.
		cacheAt(11, "refs/heads/release", "v0-rust-desktop-rust-Linux-x64-00000000-11111111", "2026-11-01T08:00:00Z", "2026-11-01T08:00:00Z"),
	}
	got := prunedIDs(planPrune(caches, testMain, map[int]bool{}))
	if want := []int64{1, 5}; !reflect.DeepEqual(got, want) {
		t.Errorf("pruned %v, want %v", got, want)
	}
}

func TestPlanPruneDropsEveryCacheOfAClosedPR(t *testing.T) {
	caches := []cacheEntry{
		cacheAt(1, "refs/pull/1365/merge", "lane-green-website-aaaa", "2026-11-23T08:00:00Z", "2026-11-23T08:00:00Z"),
		cacheAt(2, "refs/pull/1365/merge", "lane-markers-bin-Linux-X64-aaaaaaaa", "2026-11-23T08:00:00Z", "2026-11-23T08:00:00Z"),
		cacheAt(3, "refs/pull/1402/merge", "lane-green-website-bbbb", "2026-11-23T08:00:00Z", "2026-11-23T08:00:00Z"),
		// Within an open PR's scope, nothing goes, family or not.
		cacheAt(4, "refs/pull/1402/merge", "light-go-Linux-X64-aaaaaaaa", "2026-11-20T08:00:00Z", "2026-11-20T08:00:00Z"),
		cacheAt(5, "refs/pull/1402/merge", "light-go-Linux-X64-bbbbbbbb", "2026-11-23T08:00:00Z", "2026-11-23T08:00:00Z"),
	}
	if got, want := prunedIDs(planPrune(caches, testMain, map[int]bool{1402: true})), []int64{1, 2}; !reflect.DeepEqual(got, want) {
		t.Errorf("pruned %v, want %v", got, want)
	}
	// A failed open-PR lookup deletes no PR's caches.
	if got := planPrune(caches, testMain, nil); len(got) != 0 {
		t.Errorf("with no open-PR list, pruned %v, want nothing", prunedIDs(got))
	}
}

func TestParseCacheList(t *testing.T) {
	out := "12\trefs/heads/main\t2026-11-23T11:09:50Z\t2026-11-23T09:00:00Z\tv0-rust-x-aaaaaaaa\n\n" +
		"13\trefs/pull/7/merge\t2026-11-23T11:00:00Z\t2026-11-23T11:00:00Z\tlane-green-website-bbbb\n"
	got, err := parseCacheList(out)
	if err != nil {
		t.Fatal(err)
	}
	want := []cacheEntry{
		cacheAt(12, testMain, "v0-rust-x-aaaaaaaa", "2026-11-23T11:09:50Z", "2026-11-23T09:00:00Z"),
		cacheAt(13, "refs/pull/7/merge", "lane-green-website-bbbb", "2026-11-23T11:00:00Z", "2026-11-23T11:00:00Z"),
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("got %+v, want %+v", got, want)
	}
	if _, err := parseCacheList("12\trefs/heads/main\tkey-only\n"); err == nil {
		t.Error("a short line parsed; want an error, so a changed API shape deletes nothing")
	}
}

func TestParseOpenPRs(t *testing.T) {
	got, err := parseOpenPRs("1402\n1365\n\n")
	if err != nil || !reflect.DeepEqual(got, map[int]bool{1402: true, 1365: true}) {
		t.Errorf("got %v, %v", got, err)
	}
	if _, err := parseOpenPRs("abc\n"); err == nil {
		t.Error("a non-number parsed; want an error")
	}
}

// The touch action restores the plan's verified keys from fixed slots; one slot too few would
// leave a verified marker untouched, so evictable again.
func TestTouchActionHasOneSlotPerPossibleLane(t *testing.T) {
	src, err := os.ReadFile("../../.github/actions/touch-lane-markers/action.yml")
	if err != nil {
		t.Fatal(err)
	}
	slots := regexp.MustCompile(`key: \$\{\{ fromJSON\(inputs\.keys\)\[(\d+)\] \}\}`).FindAllStringSubmatch(string(src), -1)
	conds := regexp.MustCompile(`if: fromJSON\(inputs\.keys\)\[(\d+)\] != null`).FindAllStringSubmatch(string(src), -1)
	if len(slots) != touchSlots || len(conds) != touchSlots {
		t.Fatalf("%d key slots and %d guards, want %d each (`touchSlots`)", len(slots), len(conds), touchSlots)
	}
	for i := range touchSlots {
		if slots[i][1] != conds[i][1] || slots[i][1] != strconv.Itoa(i) {
			t.Errorf("slot %d reads index %s with guard %s; want %d for both", i, slots[i][1], conds[i][1], i)
		}
	}
}

//go:build cmdr_windows_on

package manifest

// The release pipeline dry run's Windows-on build (`pipeline_windowson_test.go` builds the CLI with
// `-tags cmdr_windows_on`): every version ships Windows, the Store's MSIX included, so the day-one path runs in CI against the
// v0.61.0 fixture without shipping anything. ❌ Never build a release tool with this tag; turning
// Windows on is the `ShippedOSes` line (`docs/guides/releasing.md` § Windows releases).
func init() {
	ShippedOSes = append(ShippedOSes, Shipping{OS: Windows, StorePackageSince: "0.0.0"})
}

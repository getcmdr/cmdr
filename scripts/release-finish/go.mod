module cmdr/scripts/release-finish

go 1.26.0

require golang.org/x/crypto v0.58.0

require (
	cmdr/scripts/release-assets v0.0.0
	golang.org/x/sys v0.49.0 // indirect
)

replace cmdr/scripts/release-assets => ../release-assets

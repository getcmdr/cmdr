//go:build unix

package targetsweep

import (
	"io/fs"
	"syscall"
)

// linkCount is a file's hard-link count, the signal of cargo's uplift.
func linkCount(info fs.FileInfo) uint64 {
	if st, ok := info.Sys().(*syscall.Stat_t); ok {
		return uint64(st.Nlink)
	}
	return 1
}

// fileID is a file's device and inode, which two hard links share.
func fileID(info fs.FileInfo) (fileIdentity, bool) {
	if st, ok := info.Sys().(*syscall.Stat_t); ok {
		return fileIdentity{dev: uint64(st.Dev), ino: st.Ino}, true
	}
	return fileIdentity{}, false
}

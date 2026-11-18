package main

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"cmdr/scripts/release-assets/manifest"
)

// Signing the update payloads on the laptop, and proving the signatures before and after they go
// up: steps 5–7 of the flow in `main.go`.

// sign runs the Tauri signer over each archive, writing `<archive>.sig` beside it. The key and
// its password go to the signer's environment only, read fresh from where `signingEnv` says.
func sign(cfg config, dir string, names []string) error {
	env, err := signingEnv(cfg.backupKey, os.Getenv, func(name string) (string, error) {
		return output(cfg.root, nil, "secret", name)
	})
	if err != nil {
		return err
	}
	release := manifest.For(cfg.version)
	for _, name := range names {
		path := filepath.Join(dir, name)
		if err := os.Remove(path + ".sig"); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
		if _, err := output(filepath.Join(cfg.root, "apps", "desktop"), env, "pnpm", signerArgs(release, name, path)...); err != nil {
			return fmt.Errorf("signing %s: %w", name, err)
		}
		fmt.Printf("    signed %s\n", name)
	}
	return nil
}

// signerArgs is the `pnpm` command line that signs one update payload. A payload whose row signs
// its version (Windows and Linux, `manifest.File.SignsVersion`) gets `--app-version`, which writes
// `version:<v>` into minisign's signed trusted comment: off macOS the updater binds the version
// there and refuses a signature without it. macOS archives are signed as they always were.
func signerArgs(release manifest.Release, name, path string) []string {
	args := []string{"exec", "tauri", "signer", "sign"}
	for _, f := range release.Updates() {
		if f.Name == name && f.SignsVersion() {
			args = append(args, "--app-version", release.Version)
		}
	}
	return append(args, path)
}

// signingEnv answers the signer's environment: the main key from sops, or with `-backup-key` the
// offline backup key from this process's environment, where David put it from Bitwarden for the
// one rotation release. The backup key never goes into sops.
func signingEnv(backupKey bool, getenv func(string) string, readSecret func(string) (string, error)) ([]string, error) {
	var key, password string
	if backupKey {
		step("Signing with the BACKUP updater key from %s", backupKeyEnv)
		key, password = getenv(backupKeyEnv), getenv(backupPasswordEnv)
		if key == "" || password == "" {
			return nil, fmt.Errorf("-backup-key needs %s and %s set (from Bitwarden, see docs/guides/releasing.md § Rotating to the backup updater key)", backupKeyEnv, backupPasswordEnv)
		}
	} else {
		step("Signing with the updater key from sops")
		var err error
		if key, err = readSecret(keySecret); err != nil {
			return nil, fmt.Errorf("reading %s from sops: %w", keySecret, err)
		}
		if password, err = readSecret(passwordSecret); err != nil {
			return nil, fmt.Errorf("reading %s from sops: %w", passwordSecret, err)
		}
	}
	return []string{"TAURI_SIGNING_PRIVATE_KEY=" + key, "TAURI_SIGNING_PRIVATE_KEY_PASSWORD=" + password}, nil
}

func verifySignatures(pubkeys []string, dir string, names []string) error {
	step("Verifying the signatures against the app's trusted keys")
	for _, name := range names {
		keyID, err := verifyFile(pubkeys, filepath.Join(dir, name), filepath.Join(dir, name+".sig"))
		if err != nil {
			return fmt.Errorf("the signature for %s doesn't verify against the app's trusted keys (made with the wrong key?): %w", name, err)
		}
		fmt.Printf("    %s.sig verifies (key %s)\n", name, keyID)
	}
	return nil
}

func uploadSignatures(cfg config, dir string, names []string) error {
	step("Uploading the signatures to the %s draft", cfg.tag)
	args := []string{"release", "upload", cfg.tag, "-R", repo, "--clobber"}
	for _, name := range names {
		args = append(args, filepath.Join(dir, name+".sig"))
	}
	if _, err := output(cfg.root, nil, "gh", args...); err != nil {
		return err
	}
	return checkUploadedSignatures(cfg, dir, names)
}

func verifyFile(pubkeys []string, archivePath, sigPath string) (string, error) {
	sig, err := os.ReadFile(sigPath)
	if err != nil {
		return "", err
	}
	f, err := os.Open(archivePath)
	if err != nil {
		return "", err
	}
	defer f.Close()
	return verifyTauriSignature(pubkeys, string(sig), f)
}

// checkUploadedSignatures downloads the `.sig` assets back and compares them with what was
// signed, so the manifest is built from exactly the signatures verified above.
func checkUploadedSignatures(cfg config, dir string, names []string) error {
	back := filepath.Join(dir, "uploaded")
	if err := os.MkdirAll(back, 0o755); err != nil {
		return err
	}
	for _, name := range names {
		if _, err := output(cfg.root, nil, "gh", "release", "download", cfg.tag, "-R", repo, "-p", name+".sig", "-D", back, "--clobber"); err != nil {
			return err
		}
		local, err := os.ReadFile(filepath.Join(dir, name+".sig"))
		if err != nil {
			return err
		}
		uploaded, err := os.ReadFile(filepath.Join(back, name+".sig"))
		if err != nil {
			return err
		}
		if !bytes.Equal(local, uploaded) {
			return fmt.Errorf("%s.sig on the release differs from the one just signed. Run this again", name)
		}
	}
	fmt.Println("    the release carries the signatures just verified")
	return nil
}

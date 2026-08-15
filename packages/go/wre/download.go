package wre

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
)

func isLocalhost(host string) bool {
	switch host {
	case "localhost", "127.0.0.1", "::1":
		return true
	default:
		return false
	}
}

func downloadTo(ctx context.Context, rawURL, sha256Hex, dest string) error {
	if sha256Hex == "" {
		return newError(KindBadInput, "sha256 is required to verify a download of %q", rawURL)
	}

	parsed, err := url.Parse(rawURL)
	if err != nil {
		return newError(KindBadInput, "invalid download URL %q: %v", rawURL, err)
	}
	if parsed.Scheme != "https" {
		if parsed.Scheme != "http" || !isLocalhost(parsed.Hostname()) {
			return newError(KindBadInput, "refusing non-https download URL %q", rawURL)
		}
	}

	dir := filepath.Dir(dest)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return newError(KindResource, "cannot create %q: %v", dir, err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return newError(KindBadInput, "cannot build request for %q: %v", rawURL, err)
	}

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return newError(KindResource, "download of %q failed: %v", rawURL, err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return newError(KindResource, "download of %q failed: http %d", rawURL, resp.StatusCode)
	}

	tmp, err := os.CreateTemp(dir, ".wred-download-*")
	if err != nil {
		return newError(KindResource, "cannot create temp file in %q: %v", dir, err)
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)

	h := sha256.New()
	if _, err := io.Copy(tmp, io.TeeReader(resp.Body, h)); err != nil {
		tmp.Close()
		return newError(KindResource, "writing download of %q failed: %v", rawURL, err)
	}
	if err := tmp.Close(); err != nil {
		return newError(KindResource, "closing download of %q failed: %v", rawURL, err)
	}

	actual := hex.EncodeToString(h.Sum(nil))
	if !strings.EqualFold(actual, sha256Hex) {
		return newError(KindResource, "sha256 mismatch for download of %q: expected %s, got %s", rawURL, sha256Hex, actual)
	}

	if err := os.Chmod(tmpPath, 0o755); err != nil {
		return newError(KindResource, "chmod %q failed: %v", tmpPath, err)
	}

	if err := os.Rename(tmpPath, dest); err != nil {
		return newError(KindResource, "installing binary at %q failed: %v", dest, err)
	}

	return nil
}

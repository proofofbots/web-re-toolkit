package wre

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

var tripleTable = map[string]string{
	"darwin/arm64":  "aarch64-apple-darwin",
	"darwin/amd64":  "x86_64-apple-darwin",
	"linux/amd64":   "x86_64-unknown-linux-gnu",
	"linux/arm64":   "aarch64-unknown-linux-gnu",
	"windows/amd64": "x86_64-pc-windows-msvc",
}

func CurrentTriple() (string, error) {
	key := runtime.GOOS + "/" + runtime.GOARCH
	triple, ok := tripleTable[key]
	if !ok {
		return "", newError(KindUnsupported, "no known binary triple for GOOS %q GOARCH %q", runtime.GOOS, runtime.GOARCH)
	}
	return triple, nil
}

func CacheRoot() string {
	if dir := os.Getenv("WRE_CACHE_DIR"); dir != "" {
		return dir
	}
	if runtime.GOOS == "windows" {
		if dir := os.Getenv("LOCALAPPDATA"); dir != "" {
			return filepath.Join(dir, "wre")
		}
	} else if dir := os.Getenv("XDG_CACHE_HOME"); dir != "" {
		return filepath.Join(dir, "wre")
	}
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".cache", "wre")
	}
	return filepath.Join(".cache", "wre")
}

type BinarySpec struct {
	Version string
	Triple  string
	SHA256  string
	URL     string
}

func binaryFileName() string {
	if runtime.GOOS == "windows" {
		return "wred.exe"
	}
	return "wred"
}

func VerifySHA256(path, expected string) error {
	f, err := os.Open(path)
	if err != nil {
		return newError(KindResource, "cannot open %q for sha256 verification: %v", path, err)
	}
	defer f.Close()

	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return newError(KindResource, "cannot read %q for sha256 verification: %v", path, err)
	}

	actual := hex.EncodeToString(h.Sum(nil))
	if !strings.EqualFold(actual, expected) {
		return newError(KindResource, "sha256 mismatch for %q: expected %s, got %s", path, expected, actual)
	}
	return nil
}

func ResolveBinary(spec BinarySpec) (string, error) {
	var tried []string

	if envBinary := os.Getenv("WRE_BINARY"); envBinary != "" {
		tried = append(tried, envBinary)
		if info, err := os.Stat(envBinary); err == nil && !info.IsDir() {
			return envBinary, nil
		}
	}

	triple := spec.Triple
	if triple == "" {
		t, err := CurrentTriple()
		if err != nil {
			return "", err
		}
		triple = t
	}

	cached := filepath.Join(CacheRoot(), "bin", spec.Version, triple, binaryFileName())
	tried = append(tried, cached)
	if info, err := os.Stat(cached); err == nil && !info.IsDir() {
		if spec.SHA256 != "" {
			if err := VerifySHA256(cached, spec.SHA256); err != nil {
				return "", err
			}
		}
		return cached, nil
	}

	if spec.URL != "" {
		url := strings.ReplaceAll(spec.URL, "{version}", spec.Version)
		url = strings.ReplaceAll(url, "{triple}", triple)
		tried = append(tried, url)
		if err := downloadTo(context.Background(), url, spec.SHA256, cached); err != nil {
			return "", err
		}
		return cached, nil
	}

	return "", newError(KindResource, "could not resolve a wred binary, tried: %s", strings.Join(tried, ", "))
}

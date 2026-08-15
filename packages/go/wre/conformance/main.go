package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"time"

	"github.com/proofofbots/web-re-toolkit/packages/go/wre"
)

type suite struct {
	Target string         `json:"target"`
	Config map[string]any `json:"config"`
	Cases  []testCase     `json:"cases"`
	Diag   map[string]any `json:"diag"`
}

type testCase struct {
	Name        string         `json:"name"`
	Op          string         `json:"op"`
	Params      map[string]any `json:"params"`
	Expect      any            `json:"expect"`
	ExpectKeys  []string       `json:"expect_keys"`
	ExpectError string         `json:"expect_error"`
	DeadlineMs  int64          `json:"deadline_ms"`
}

type result struct {
	Name    string `json:"name"`
	OK      bool   `json:"ok"`
	Problem string `json:"problem,omitempty"`
}

type summary struct {
	Language string   `json:"language"`
	Target   string   `json:"target"`
	Passed   int      `json:"passed"`
	Failed   int      `json:"failed"`
	Cases    []result `json:"cases"`
}

func check(item testCase, raw json.RawMessage, err error) string {
	if err != nil {
		var typed *wre.Error
		kind := "unknown"
		message := err.Error()
		if errors.As(err, &typed) {
			kind = typed.Kind
			message = typed.Message
		}

		if item.ExpectError == "" {
			return fmt.Sprintf("failed: %s %s", kind, message)
		}
		if kind != item.ExpectError {
			return fmt.Sprintf("expected %s, got %s: %s", item.ExpectError, kind, message)
		}
		return ""
	}

	if item.ExpectError != "" {
		return fmt.Sprintf("expected %s, the call succeeded", item.ExpectError)
	}

	var value any
	if len(raw) > 0 {
		if err := json.Unmarshal(raw, &value); err != nil {
			return fmt.Sprintf("result was not json: %v", err)
		}
	}

	if item.Expect != nil {
		if wanted, ok := item.Expect.(map[string]any); ok {
			found, isObject := value.(map[string]any)
			if !isObject {
				return "result is not an object"
			}
			for key, expected := range wanted {
				actual, present := found[key]
				if !present {
					return fmt.Sprintf("%s is missing from the result", key)
				}
				if !reflect.DeepEqual(actual, expected) {
					return fmt.Sprintf("%s is %v, expected %v", key, actual, expected)
				}
			}
		} else if !reflect.DeepEqual(value, item.Expect) {
			return fmt.Sprintf("result is %v, expected %v", value, item.Expect)
		}
	}

	for _, key := range item.ExpectKeys {
		found, isObject := value.(map[string]any)
		if !isObject {
			return "result is not an object"
		}
		if _, present := found[key]; !present {
			return fmt.Sprintf("%s is missing from the result", key)
		}
	}

	return ""
}

func report(out summary) {
	encoded, _ := json.Marshal(out)
	os.Stdout.Write(encoded)
}

func fail(problem string) {
	report(summary{
		Language: "go",
		Target:   "unknown",
		Passed:   0,
		Failed:   1,
		Cases:    []result{{Name: "harness", OK: false, Problem: problem}},
	})
	os.Exit(1)
}

func main() {
	path := filepath.Join("..", "..", "..", "conformance", "example.json")
	if len(os.Args) > 1 {
		path = os.Args[1]
	}

	body, err := os.ReadFile(path)
	if err != nil {
		fail(err.Error())
	}

	var parsed suite
	if err := json.Unmarshal(body, &parsed); err != nil {
		fail(err.Error())
	}

	ctx := context.Background()

	sidecar, err := wre.Connect(ctx, wre.Options{
		Binary: os.Getenv("WRE_BINARY"),
		Stderr: io.Discard,
	})
	if err != nil {
		fail(err.Error())
	}
	defer sidecar.Close()

	session, err := sidecar.Open(ctx, parsed.Target, parsed.Config)
	if err != nil {
		fail(err.Error())
	}

	out := summary{Language: "go", Target: parsed.Target}

	for _, item := range parsed.Cases {
		deadline := item.DeadlineMs
		if deadline == 0 {
			deadline = 60000
		}
		callCtx, cancel := context.WithTimeout(ctx, time.Duration(deadline)*time.Millisecond)
		raw, callErr := session.CallRaw(callCtx, item.Op, item.Params)
		cancel()

		problem := check(item, raw, callErr)

		if problem == "" {
			out.Passed++
			out.Cases = append(out.Cases, result{Name: item.Name, OK: true})
		} else {
			out.Failed++
			out.Cases = append(out.Cases, result{Name: item.Name, OK: false, Problem: problem})
		}
	}

	closeCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
	_ = session.Close(closeCtx)
	cancel()

	report(out)

	if out.Failed > 0 {
		os.Exit(1)
	}
}

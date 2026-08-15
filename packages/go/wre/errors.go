package wre

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

const (
	KindBadInput    = "bad_input"
	KindUnsupported = "unsupported"
	KindTargetDrift = "target_drift"
	KindBlocked     = "blocked"
	KindTimeout     = "timeout"
	KindCancelled   = "cancelled"
	KindResource    = "resource"
	KindProtocol    = "protocol"
	KindInternal    = "internal"
)

type Error struct {
	Kind      string
	Message   string
	Retryable bool
	Target    string
	Op        string
	Detail    json.RawMessage
}

func (e *Error) Error() string {
	var b strings.Builder
	b.WriteString(e.Kind)
	loc := e.locator()
	if loc != "" {
		b.WriteString(" in ")
		b.WriteString(loc)
	}
	if e.Message != "" {
		b.WriteString(": ")
		b.WriteString(e.Message)
	}
	return b.String()
}

func (e *Error) locator() string {
	switch {
	case e.Target != "" && e.Op != "":
		return e.Target + "." + e.Op
	case e.Target != "":
		return e.Target
	case e.Op != "":
		return e.Op
	default:
		return ""
	}
}

func defaultRetryable(kind string) bool {
	switch kind {
	case KindBlocked, KindTimeout, KindResource:
		return true
	default:
		return false
	}
}

func newError(kind, format string, args ...any) *Error {
	return &Error{
		Kind:      kind,
		Message:   fmt.Sprintf(format, args...),
		Retryable: defaultRetryable(kind),
	}
}

func IsKind(err error, kind string) bool {
	var e *Error
	if errors.As(err, &e) {
		return e.Kind == kind
	}
	return false
}

func errorFromWire(raw json.RawMessage) *Error {
	if len(raw) == 0 {
		return &Error{Kind: KindInternal, Message: "unknown error"}
	}
	var wire struct {
		Kind      string          `json:"kind"`
		Message   string          `json:"message"`
		Retryable bool            `json:"retryable"`
		Target    string          `json:"target"`
		Op        string          `json:"op"`
		Detail    json.RawMessage `json:"detail"`
	}
	if err := json.Unmarshal(raw, &wire); err != nil {
		return &Error{Kind: KindInternal, Message: "malformed error payload: " + err.Error()}
	}
	kind := wire.Kind
	if kind == "" {
		kind = KindInternal
	}
	message := wire.Message
	if message == "" {
		message = "unknown error"
	}
	return &Error{
		Kind:      kind,
		Message:   message,
		Retryable: wire.Retryable,
		Target:    wire.Target,
		Op:        wire.Op,
		Detail:    wire.Detail,
	}
}

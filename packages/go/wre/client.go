package wre

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"sync"
	"time"
)

type Options struct {
	Binary           string
	Args             []string
	Env              []string
	Dir              string
	Stderr           io.Writer
	OnEvent          func(id uint64, event string, data json.RawMessage)
	ExpectProtocol   int
	ExpectSchemaHash string
	StartupTimeout   time.Duration
}

type Hello struct {
	Protocol       int      `json:"protocol"`
	Bundle         string   `json:"bundle"`
	BinaryVersion  string   `json:"binary_version"`
	ToolkitVersion string   `json:"toolkit_version"`
	SchemaHash     string   `json:"schema_hash"`
	Targets        []string `json:"targets"`
	Workers        int      `json:"workers"`
	Pid            int      `json:"pid"`
}

type Health struct {
	OK     bool            `json:"ok"`
	Target string          `json:"target"`
	Detail json.RawMessage `json:"detail"`
}

type inboundResult struct {
	ok     bool
	result json.RawMessage
	errRaw json.RawMessage
	bin    []byte
	err    error
}

type Sidecar struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout io.ReadCloser

	writeMu sync.Mutex

	mu       sync.Mutex
	pending  map[uint64]chan *inboundResult
	nextID   uint64
	closed   bool
	closeErr *Error

	hello   Hello
	onEvent func(id uint64, event string, data json.RawMessage)

	readDone  chan struct{}
	closeOnce sync.Once
}

const defaultStartupTimeout = 10 * time.Second

func stderrWriter(requested io.Writer) io.Writer {
	switch os.Getenv("WRE_STDERR") {
	case "inherit":
		return os.Stderr
	case "ignore":
		return io.Discard
	}
	if requested == nil {
		return io.Discard
	}
	return requested
}

func Connect(ctx context.Context, opts Options) (*Sidecar, error) {
	if opts.Binary == "" {
		return nil, newError(KindBadInput, "Options.Binary is required")
	}

	expectProtocol := opts.ExpectProtocol
	if expectProtocol == 0 {
		expectProtocol = protocolVersion
	}
	startupTimeout := opts.StartupTimeout
	if startupTimeout <= 0 {
		startupTimeout = defaultStartupTimeout
	}

	args := append([]string{"--stdio"}, opts.Args...)
	cmd := exec.Command(opts.Binary, args...)
	cmd.Dir = opts.Dir
	if len(opts.Env) > 0 {
		cmd.Env = opts.Env
	}
	cmd.Stderr = stderrWriter(opts.Stderr)

	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, newError(KindInternal, "cannot open stdin pipe: %v", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, newError(KindInternal, "cannot open stdout pipe: %v", err)
	}

	if err := cmd.Start(); err != nil {
		return nil, newError(KindResource, "cannot start %q: %v", opts.Binary, err)
	}

	s := &Sidecar{
		cmd:      cmd,
		stdin:    stdin,
		stdout:   stdout,
		pending:  make(map[uint64]chan *inboundResult),
		onEvent:  opts.OnEvent,
		readDone: make(chan struct{}),
	}

	go s.readLoop()

	helloCtx := ctx
	if _, ok := ctx.Deadline(); !ok {
		var cancel context.CancelFunc
		helloCtx, cancel = context.WithTimeout(ctx, startupTimeout)
		defer cancel()
	}

	raw, err := s.Call(helloCtx, "hello", struct{}{}, "")
	if err != nil {
		s.Close()
		return nil, err
	}

	var hello Hello
	if err := json.Unmarshal(raw, &hello); err != nil {
		s.Close()
		return nil, newError(KindProtocol, "cannot decode hello response: %v", err)
	}

	if hello.Protocol != expectProtocol {
		s.Close()
		return nil, newError(KindProtocol, "protocol mismatch: expected %d, got %d", expectProtocol, hello.Protocol)
	}
	if opts.ExpectSchemaHash != "" && hello.SchemaHash != opts.ExpectSchemaHash {
		s.Close()
		return nil, newError(KindProtocol, "schema hash mismatch: expected %s, got %s", opts.ExpectSchemaHash, hello.SchemaHash)
	}

	s.hello = hello
	return s, nil
}

func (s *Sidecar) readLoop() {
	defer close(s.readDone)
	for {
		f, err := readFrame(s.stdout)
		if err != nil {
			if err == io.EOF {
				s.failAll(newError(KindProtocol, "sidecar closed the connection"))
			} else if pe, ok := err.(*Error); ok {
				s.failAll(pe)
			} else {
				s.failAll(newError(KindProtocol, "sidecar read failed: %v", err))
			}
			return
		}

		var peek struct {
			T string `json:"t"`
		}
		if err := json.Unmarshal(f.json, &peek); err != nil {
			s.failAll(newError(KindProtocol, "malformed envelope: %v", err))
			return
		}

		switch peek.T {
		case "res":
			var env resEnvelope
			if err := json.Unmarshal(f.json, &env); err != nil {
				s.failAll(newError(KindProtocol, "malformed response envelope: %v", err))
				return
			}
			s.deliver(env.ID, &inboundResult{ok: env.OK, result: env.Result, errRaw: env.Error, bin: f.bin})
		case "evt":
			var env evtEnvelope
			if err := json.Unmarshal(f.json, &env); err != nil {
				continue
			}
			if s.onEvent != nil {
				s.onEvent(env.ID, env.Event, env.Data)
			}
		}
	}
}

func (s *Sidecar) deliver(id uint64, res *inboundResult) {
	s.mu.Lock()
	ch, ok := s.pending[id]
	if ok {
		delete(s.pending, id)
	}
	s.mu.Unlock()
	if ok {
		ch <- res
	}
}

func (s *Sidecar) failAll(err *Error) {
	s.mu.Lock()
	if !s.closed {
		s.closed = true
		s.closeErr = err
	}
	pending := s.pending
	s.pending = make(map[uint64]chan *inboundResult)
	s.mu.Unlock()

	for _, ch := range pending {
		ch <- &inboundResult{err: err}
	}
}

func (s *Sidecar) sendCancel(id uint64) {
	raw, err := json.Marshal(cancelEnvelope{T: "cancel", V: protocolVersion, ID: id})
	if err != nil {
		return
	}
	s.writeMu.Lock()
	_ = writeFrame(s.stdin, frame{json: raw})
	s.writeMu.Unlock()
}

func (s *Sidecar) roundTrip(ctx context.Context, op string, params any, session string, bin []byte) (json.RawMessage, []byte, error) {
	s.mu.Lock()
	if s.closed {
		err := s.closeErr
		s.mu.Unlock()
		if err == nil {
			err = newError(KindProtocol, "sidecar is closed")
		}
		return nil, nil, err
	}
	s.nextID++
	id := s.nextID
	ch := make(chan *inboundResult, 1)
	s.pending[id] = ch
	s.mu.Unlock()

	paramsRaw, err := json.Marshal(params)
	if err != nil {
		s.mu.Lock()
		delete(s.pending, id)
		s.mu.Unlock()
		return nil, nil, newError(KindBadInput, "cannot encode params for %q: %v", op, err)
	}

	req := reqEnvelope{
		T:       "req",
		V:       protocolVersion,
		ID:      id,
		Op:      op,
		Session: session,
		Params:  paramsRaw,
	}
	if dl, ok := ctx.Deadline(); ok {
		ms := time.Until(dl).Milliseconds()
		if ms < 0 {
			ms = 0
		}
		req.DeadlineMs = ms
	}

	reqJSON, err := json.Marshal(req)
	if err != nil {
		s.mu.Lock()
		delete(s.pending, id)
		s.mu.Unlock()
		return nil, nil, newError(KindInternal, "cannot encode request envelope: %v", err)
	}

	s.writeMu.Lock()
	writeErr := writeFrame(s.stdin, frame{json: reqJSON, bin: bin})
	s.writeMu.Unlock()
	if writeErr != nil {
		s.mu.Lock()
		delete(s.pending, id)
		s.mu.Unlock()
		return nil, nil, newError(KindProtocol, "cannot write request for %q: %v", op, writeErr)
	}

	select {
	case res := <-ch:
		if res.err != nil {
			return nil, nil, res.err
		}
		if !res.ok {
			return nil, nil, errorFromWire(res.errRaw)
		}
		return res.result, res.bin, nil
	case <-ctx.Done():
		s.sendCancel(id)
		kind := KindCancelled
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			kind = KindTimeout
		}
		cancelErr := newError(kind, "%s: %v", op, ctx.Err())
		cancelErr.Op = op
		return nil, nil, cancelErr
	}
}

func (s *Sidecar) Hello() Hello {
	return s.hello
}

func (s *Sidecar) Describe(ctx context.Context) (json.RawMessage, error) {
	return s.Call(ctx, "describe", struct{}{}, "")
}

func (s *Sidecar) Targets(ctx context.Context) ([]string, error) {
	raw, err := s.Call(ctx, "targets", struct{}{}, "")
	if err != nil {
		return nil, err
	}
	var targets []string
	if err := json.Unmarshal(raw, &targets); err != nil {
		return nil, newError(KindProtocol, "cannot decode targets result: %v", err)
	}
	return targets, nil
}

func (s *Sidecar) Metrics(ctx context.Context) (json.RawMessage, error) {
	return s.Call(ctx, "metrics", struct{}{}, "")
}

func (s *Sidecar) Open(ctx context.Context, target string, config any) (*Session, error) {
	params := map[string]any{"target": target, "config": config}
	raw, err := s.Call(ctx, "open", params, "")
	if err != nil {
		return nil, err
	}
	var opened struct {
		Session string   `json:"session"`
		Target  string   `json:"target"`
		Worker  int      `json:"worker"`
		Ops     []string `json:"ops"`
	}
	if err := json.Unmarshal(raw, &opened); err != nil {
		return nil, newError(KindProtocol, "cannot decode open result: %v", err)
	}
	return &Session{
		sidecar: s,
		id:      opened.Session,
		target:  opened.Target,
		ops:     opened.Ops,
	}, nil
}

func (s *Sidecar) Call(ctx context.Context, op string, params any, session string) (json.RawMessage, error) {
	result, _, err := s.roundTrip(ctx, op, params, session, nil)
	return result, err
}

func (s *Sidecar) CallBinary(ctx context.Context, op string, params any, session string, bin []byte) (json.RawMessage, []byte, error) {
	return s.roundTrip(ctx, op, params, session, bin)
}

func (s *Sidecar) Shutdown(ctx context.Context) error {
	_, err := s.Call(ctx, "shutdown", struct{}{}, "")
	return err
}

func (s *Sidecar) Close() error {
	s.closeOnce.Do(func() {
		if s.cmd.Process != nil {
			_ = s.cmd.Process.Kill()
		}
		_ = s.stdin.Close()
		<-s.readDone
		_ = s.cmd.Wait()
		s.failAll(newError(KindProtocol, "sidecar closed"))
	})
	return nil
}

type Session struct {
	sidecar *Sidecar
	id      string
	target  string
	ops     []string

	closeOnce sync.Once
	closeErr  error
}

func (sess *Session) ID() string {
	return sess.id
}

func (sess *Session) Target() string {
	return sess.target
}

func (sess *Session) Ops() []string {
	return sess.ops
}

func (sess *Session) CallRaw(ctx context.Context, op string, params any) (json.RawMessage, error) {
	return sess.sidecar.Call(ctx, op, params, sess.id)
}

func (sess *Session) Call(ctx context.Context, op string, params any, out any) error {
	raw, err := sess.CallRaw(ctx, op, params)
	if err != nil {
		return err
	}
	if out == nil {
		return nil
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return newError(KindProtocol, "cannot decode result for %q: %v", op, err)
	}
	return nil
}

func (sess *Session) CallBinary(ctx context.Context, op string, params any, bin []byte) (json.RawMessage, []byte, error) {
	return sess.sidecar.CallBinary(ctx, op, params, sess.id, bin)
}

func (sess *Session) Warmup(ctx context.Context) error {
	_, err := sess.CallRaw(ctx, "warmup", struct{}{})
	return err
}

func (sess *Session) Health(ctx context.Context) (Health, error) {
	raw, err := sess.CallRaw(ctx, "health", struct{}{})
	if err != nil {
		return Health{}, err
	}
	var health Health
	if err := json.Unmarshal(raw, &health); err != nil {
		return Health{}, newError(KindProtocol, "cannot decode health result: %v", err)
	}
	return health, nil
}

func (sess *Session) Close(ctx context.Context) error {
	sess.closeOnce.Do(func() {
		_, err := sess.CallRaw(ctx, "close", struct{}{})
		sess.closeErr = err
	})
	return sess.closeErr
}

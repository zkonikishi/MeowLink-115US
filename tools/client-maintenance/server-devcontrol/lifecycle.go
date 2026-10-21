package devcontrol

import (
	"bytes"
	"context"
	"encoding/json"
	"sync"
	"time"
)

const ManagementPath = "/mods/dev.meowscope/devtest"

// ManagementHost is installed in process by the host. Resolve must recover a
// private management session from trusted context, never from request JSON.
// Authorizer still independently verifies issuer, revocation and permissions.
type ManagementHost struct {
	Authorizer Authorizer
	Resolve    func(context.Context) (HostProof, error)
	Scenarios  []ScenarioRegistration
}

// Lifecycle owns one controller for one Mod lifetime. Its zero value is closed.
// Boot is intentionally single use: an unfinished shutdown cannot be bypassed
// by booting a second executor in the same runtime.
type Lifecycle struct {
	mu             sync.RWMutex
	booted, closed bool
	controller     *Controller
	resolve        func(context.Context) (HostProof, error)
}

func (l *Lifecycle) Boot(ctx context.Context, host *ManagementHost) error {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.booted || l.closed {
		return nil
	}
	l.booted = true
	if host == nil || host.Resolve == nil {
		return nil
	}
	c, err := New(ctx, host.Authorizer, host.Scenarios)
	if err != nil {
		return err
	}
	l.controller, l.resolve = c, host.Resolve
	return nil
}

// Dispatch is an exact-path adapter for the existing localgm executor. A false
// match delegates to its existing handler; no listener or player connection is
// created. Status codes and replies contain no provider error text.
func (l *Lifecycle) Dispatch(ctx context.Context, path string, body json.RawMessage) (matched bool, status int, reply Reply) {
	if path != ManagementPath {
		return false, 0, Reply{}
	}
	matched, status = true, 503
	reply = Reply{Code: "management_session_unavailable"}
	defer func() {
		if recover() != nil {
			status = 503
			reply = Reply{Code: "management_session_unavailable"}
		}
	}()
	l.mu.RLock()
	c, resolve, closed := l.controller, l.resolve, l.closed
	l.mu.RUnlock()
	if closed || c == nil || resolve == nil {
		return
	}
	proof, err := resolve(ctx)
	if err != nil || isNil(proof) {
		return
	}
	req, err := DecodeRequest(bytes.NewReader(body))
	if err != nil {
		return true, 400, Reply{Code: "invalid_request"}
	}
	reply = c.Handle(ctx, proof, req)
	status = ReplyStatus(reply)
	return
}

// ReplyStatus preserves the distinction between permission, missing adapters,
// disabled tests and invalid plans. Unknown failures stay server errors.
func ReplyStatus(reply Reply) int {
	if reply.OK {
		return 200
	}
	switch reply.Code {
	case "denied", "isolated_adapter_required":
		return 403
	case "invalid_request", "scenario_budget_mismatch":
		return 400
	case "run_not_found", "scenario_not_registered":
		return 404
	case "unsupported":
		return 501
	case "disabled", "busy", "closed", "enable_rejected", "start_rejected":
		return 409
	case "request_cancelled":
		return 408
	case "management_session_unavailable", "shutdown_pending":
		return 503
	default:
		return 500
	}
}

// Shutdown is bounded even when a fixture ignores cancellation. The controller
// retains its pending cleanup state; the caller must not claim forced cleanup.
func (l *Lifecycle) Shutdown(ctx context.Context) Reply {
	l.mu.Lock()
	l.closed = true
	c := l.controller
	l.mu.Unlock()
	if c == nil {
		return Reply{OK: true, Code: "shutdown"}
	}
	bounded, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	return c.Shutdown(bounded)
}

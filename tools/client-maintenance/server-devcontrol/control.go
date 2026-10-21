// Package devcontrol is MeowScope's server-side GM/AI control adapter. It owns
// no listener, account issuer, arbitrary shell/SQL/URL executor or game client.
package devcontrol

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	devtest "meowlink/devtest"
	"reflect"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
)

type Operation string

const (
	List   Operation = "list"
	Start  Operation = "start"
	Status Operation = "status"
	Cancel Operation = "cancel"
	Report Operation = "report"
	Enable Operation = "enable"
)

type PrincipalKind uint8

const (
	GM PrincipalKind = iota + 1
	AI
)

// Marker alone is NOT authentication. The actual host Authorizer MUST assert
// its private concrete issuer/session/RBAC binding and current revocation state.
type HostProof interface{ MeowScopeControlOpaque() }
type HostGrant struct {
	Owner             [32]byte
	Kind              PrincipalKind
	Lifetime          context.Context // management session, not the current HTTP request
	IsolationApproved bool
	ManageAll         bool
}
type Authorizer interface {
	Authorize(context.Context, Operation, HostProof) (HostGrant, error)
}

// JSON has no actor, UID, owner, role, target URL, script, SQL or credential.
type Request struct {
	Op          Operation `json:"op"`
	Scenario    string    `json:"scenario,omitempty"`
	RunID       string    `json:"run_id,omitempty"`
	Concurrency int       `json:"concurrency,omitempty"`
	Rate        int       `json:"rate,omitempty"`
	Calls       int       `json:"calls,omitempty"`
	DurationMS  int       `json:"duration_ms,omitempty"`
	Enabled     *bool     `json:"enabled,omitempty"`
}

// Server-owned registration only, never decoded from a GM/player request.
// The private adapter must verify its isolated target/test identities and exact
// network profile. A public Layer/name/boolean is not native protocol evidence.
type NetworkCapability struct {
	Verified, Isolated          bool
	ProfileSHA256, TargetSHA256 [32]byte
}
type NetworkAdapter interface {
	Capability() NetworkCapability
	Prepare(context.Context) (devtest.Fixture, error)
}
type ScenarioRegistration struct {
	Scenario                                    devtest.Scenario
	ValidationLevel                             string // optional assertion; actual level derives from Layer
	Network                                     NetworkAdapter
	ExpectedProfileSHA256, ExpectedTargetSHA256 [32]byte
	ExactConcurrency, ExactCalls                int // optional trusted adapter constraints
}
type ScenarioInfo struct {
	Name                    string `json:"name"`
	Layer                   string `json:"layer"`
	ValidationLevel         string `json:"validation_level"`
	Runnable                bool   `json:"runnable"`
	Accounts                int    `json:"accounts"`
	AuthMode                string `json:"auth_mode,omitempty"`
	Profile                 string `json:"profile,omitempty"`
	NetworkEvidenceApproved bool   `json:"network_evidence_approved"`
	BaseValidationLevel     string `json:"base_validation_level"`
	ExactConcurrency        int    `json:"exact_concurrency,omitempty"`
	ExactCalls              int    `json:"exact_calls,omitempty"`
}
type RunReport struct {
	ID                      string `json:"run_id,omitempty"`
	Scenario                string `json:"scenario,omitempty"`
	Layer                   string `json:"layer,omitempty"`
	ValidationLevel         string `json:"validation_level,omitempty"`
	Accounts                int    `json:"accounts"`
	AuthMode                string `json:"auth_mode,omitempty"`
	Profile                 string `json:"profile,omitempty"`
	NetworkEvidenceApproved bool   `json:"network_evidence_approved"`
	State                   string `json:"state"`
	StopReason              string `json:"stop_reason,omitempty"`
	StartedMS               int64  `json:"started_ms,omitempty"`
	FinishedMS              int64  `json:"finished_ms,omitempty"`
	Attempts                int    `json:"attempts"`
	Passed                  int    `json:"passed"`
	Failed                  int    `json:"failed"`
	PeakConcurrency         int    `json:"peak_concurrency"`
	P50US                   int64  `json:"p50_us"`
	P95US                   int64  `json:"p95_us"`
	P99US                   int64  `json:"p99_us"`
	P50NS                   int64  `json:"p50_ns"`
	P95NS                   int64  `json:"p95_ns"`
	P99NS                   int64  `json:"p99_ns"`
	CleanupFailed           bool   `json:"cleanup_failed"`
	CleanupPending          bool   `json:"cleanup_pending"`
	Outcome                 string `json:"outcome"`
	OverallPassed           bool   `json:"overall_passed"`
	NativeGameplayVerified  bool   `json:"native_gameplay_verified"`
}
type Reply struct {
	OK        bool           `json:"ok"`
	Code      string         `json:"code"`
	Enabled   bool           `json:"enabled"`
	Scenarios []ScenarioInfo `json:"scenarios,omitempty"`
	RunID     string         `json:"run_id,omitempty"`
	Report    *RunReport     `json:"report,omitempty"`
}
type gateKey struct{}
type kernelGrant struct {
	controller *Controller
	operation  string
	host       HostGrant
}
type Controller struct {
	mu              sync.Mutex
	kernel          *devtest.Controller
	authorizer      Authorizer
	life            context.Context
	closeLife       context.CancelFunc
	catalog         map[string]ScenarioInfo
	network         map[string]ScenarioRegistration
	enabled, closed bool
	runID           string
	plannedCalls    int
	runInfo         ScenarioInfo
	owner           [32]byte
	runCleanup      func()
}

var scenarioName = regexp.MustCompile(`^[a-z][a-z0-9_.-]{0,63}$`)
var runName = regexp.MustCompile(`^[a-zA-Z0-9_-]{1,64}$`)
var metadataName = regexp.MustCompile(`^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,95}$`)

func DecodeRequest(reader io.Reader) (Request, error) {
	var req Request
	b, err := io.ReadAll(io.LimitReader(reader, 4097))
	if err != nil || len(b) > 4096 {
		return req, errors.New("invalid developer control request")
	}
	d := json.NewDecoder(strings.NewReader(string(b)))
	token, err := d.Token()
	if err != nil || token != json.Delim('{') {
		return req, errors.New("invalid developer control request")
	}
	seen := map[string]bool{}
	for d.More() {
		token, err = d.Token()
		key, ok := token.(string)
		if err != nil || !ok || seen[key] {
			return Request{}, errors.New("invalid developer control request")
		}
		seen[key] = true
		var raw json.RawMessage
		if d.Decode(&raw) != nil || bytes.Equal(raw, []byte("null")) {
			return Request{}, errors.New("invalid developer control request")
		}
		switch key {
		case "op":
			err = json.Unmarshal(raw, &req.Op)
		case "scenario":
			err = json.Unmarshal(raw, &req.Scenario)
		case "run_id":
			err = json.Unmarshal(raw, &req.RunID)
		case "concurrency":
			err = json.Unmarshal(raw, &req.Concurrency)
		case "rate":
			err = json.Unmarshal(raw, &req.Rate)
		case "calls":
			err = json.Unmarshal(raw, &req.Calls)
		case "duration_ms":
			err = json.Unmarshal(raw, &req.DurationMS)
		case "enabled":
			err = json.Unmarshal(raw, &req.Enabled)
		default:
			return Request{}, errors.New("invalid developer control request")
		}
		if err != nil {
			return Request{}, errors.New("invalid developer control request")
		}
	}
	if token, err = d.Token(); err != nil || token != json.Delim('}') {
		return Request{}, errors.New("invalid developer control request")
	}
	var extra any
	if d.Decode(&extra) != io.EOF {
		return Request{}, errors.New("invalid developer control request")
	}
	if !valid(req) {
		return Request{}, errors.New("invalid developer control request")
	}
	for key := range seen {
		allowed := key == "op"
		switch req.Op {
		case Enable:
			allowed = allowed || key == "enabled"
		case Start:
			allowed = allowed || key == "scenario" || key == "concurrency" || key == "rate" || key == "calls" || key == "duration_ms"
		case Status, Report, Cancel:
			allowed = allowed || key == "run_id"
		}
		if !allowed {
			return Request{}, errors.New("invalid developer control request")
		}
	}
	return req, nil
}
func isNil(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Ptr, reflect.Interface, reflect.Func, reflect.Map, reflect.Slice, reflect.Chan:
		return v.IsNil()
	}
	return false
}
func valid(r Request) bool {
	switch r.Op {
	case List:
		return r.Scenario == "" && r.RunID == "" && r.Concurrency == 0 && r.Rate == 0 && r.Calls == 0 && r.DurationMS == 0 && r.Enabled == nil
	case Enable:
		return r.Enabled != nil && r.Scenario == "" && r.RunID == "" && r.Concurrency == 0 && r.Rate == 0 && r.Calls == 0 && r.DurationMS == 0
	case Start:
		return scenarioName.MatchString(r.Scenario) && r.RunID == "" && r.Enabled == nil && r.Concurrency >= 1 && r.Concurrency <= 4 && r.Rate >= 1 && r.Rate <= 100 && r.Calls >= 1 && r.Calls <= 1000 && r.DurationMS >= 1 && r.DurationMS <= 30000
	case Status, Report, Cancel:
		return (r.RunID == "" && r.Op == Status || runName.MatchString(r.RunID)) && r.Scenario == "" && r.Enabled == nil && r.Concurrency == 0 && r.Rate == 0 && r.Calls == 0 && r.DurationMS == 0
	}
	return false
}
func New(lifetime context.Context, authorize Authorizer, scenarios []ScenarioRegistration) (*Controller, error) {
	if lifetime == nil || lifetime.Err() != nil || isNil(authorize) {
		return nil, errors.New("trusted developer control host required")
	}
	life, cancel := context.WithCancel(lifetime)
	c := &Controller{authorizer: authorize, life: life, closeLife: cancel, catalog: make(map[string]ScenarioInfo), network: make(map[string]ScenarioRegistration)}
	registered := make([]devtest.Scenario, 0, len(scenarios))
	for _, r := range scenarios {
		level := map[string]string{"domain": "fixture", "host": "inprocess", "transport": "network"}[r.Scenario.Layer]
		baseLevel := level
		if r.ExactConcurrency < 0 || r.ExactConcurrency > 4 || r.ExactCalls < 0 || r.ExactCalls > 1000 {
			cancel()
			return nil, errors.New("invalid adapter plan constraints")
		}
		if !scenarioName.MatchString(r.Scenario.Name) || !r.Scenario.Isolated || level == "" || r.Scenario.Accounts < 0 || r.Scenario.Accounts > 64 || r.Scenario.AuthMode != "" && !metadataName.MatchString(r.Scenario.AuthMode) || r.Scenario.Profile != "" && !metadataName.MatchString(r.Scenario.Profile) {
			cancel()
			return nil, errors.New("invalid trusted scenario registration")
		}
		runnable := true
		if level == "network" || !isNil(r.Network) {
			c.network[r.Scenario.Name] = r
			runnable = networkReady(r)
			if r.Network != nil {
				r.Scenario.Prepare = r.Network.Prepare
			} else {
				r.Scenario.Prepare = func(context.Context) (devtest.Fixture, error) { return nil, devtest.ErrUnsupported }
			}
		}
		networkApproved := runnable && !isNil(r.Network)
		if networkApproved {
			level = "network"
		}
		if r.ValidationLevel != "" && r.ValidationLevel != level {
			cancel()
			return nil, errors.New("invalid validation-level assertion")
		}
		c.catalog[r.Scenario.Name] = ScenarioInfo{Name: r.Scenario.Name, Layer: r.Scenario.Layer, ValidationLevel: level, BaseValidationLevel: baseLevel, Runnable: runnable, Accounts: r.Scenario.Accounts, AuthMode: r.Scenario.AuthMode, Profile: r.Scenario.Profile, NetworkEvidenceApproved: networkApproved, ExactConcurrency: r.ExactConcurrency, ExactCalls: r.ExactCalls}
		registered = append(registered, r.Scenario)
	}
	kernel, err := devtest.New(c.kernelAuthorize, registered)
	if err != nil {
		cancel()
		return nil, errors.New("developer executor registration rejected")
	}
	c.kernel = kernel
	return c, nil
}
func networkReady(r ScenarioRegistration) (ready bool) {
	defer func() {
		if recover() != nil {
			ready = false
		}
	}()
	if r.Network == nil || r.ExpectedProfileSHA256 == [32]byte{} || r.ExpectedTargetSHA256 == [32]byte{} {
		return false
	}
	cap := r.Network.Capability()
	return cap.Verified && cap.Isolated && cap.ProfileSHA256 == r.ExpectedProfileSHA256 && cap.TargetSHA256 == r.ExpectedTargetSHA256
}
func (c *Controller) kernelAuthorize(ctx context.Context, operation string) error {
	grant, ok := ctx.Value(gateKey{}).(kernelGrant)
	if !ok || grant.controller != c || grant.operation != operation || grant.host.Owner == [32]byte{} || grant.host.Lifetime == nil || grant.host.Lifetime.Err() != nil || c.life.Err() != nil {
		return errors.New("trusted control grant required")
	}
	return nil
}
func (c *Controller) kernelContext(ctx context.Context, operation string, g HostGrant) context.Context {
	return context.WithValue(ctx, gateKey{}, kernelGrant{c, operation, g})
}
func (c *Controller) authorize(ctx context.Context, op Operation, proof HostProof) (g HostGrant, ok bool) {
	defer func() {
		if recover() != nil {
			ok = false
		}
	}()
	if ctx == nil || ctx.Err() != nil || isNil(proof) {
		return g, false
	}
	grant, err := c.authorizer.Authorize(ctx, op, proof)
	if err != nil || grant.Owner == [32]byte{} || (grant.Kind != GM && grant.Kind != AI) || grant.Lifetime == nil || grant.Lifetime.Err() != nil {
		return g, false
	}
	return grant, true
}
func (c *Controller) failure(code string) Reply { return Reply{Code: code, Enabled: c.enabled} }
func (c *Controller) Handle(ctx context.Context, proof HostProof, request Request) (out Reply) {
	// Fixed replies only: host/provider errors may contain secrets, never expose them.
	defer func() {
		if recover() != nil {
			out = Reply{Code: "control_adapter_fault"}
		}
	}()
	if c == nil || !valid(request) {
		return Reply{Code: "invalid_request"}
	}
	grant, ok := c.authorize(ctx, request.Op, proof)
	if !ok {
		return Reply{Code: "denied"}
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if ctx.Err() != nil {
		return c.failure("request_cancelled")
	}
	if c.closed || c.life.Err() != nil {
		return c.failure("closed")
	}
	switch request.Op {
	case List:
		names, err := c.kernel.List(c.kernelContext(ctx, "list", grant))
		if err != nil {
			return c.failure("denied")
		}
		info := make([]ScenarioInfo, 0, len(names))
		for _, name := range names {
			item := c.catalog[name]
			if registered, ok := c.network[name]; ok {
				item.Runnable = networkReady(registered)
				item.NetworkEvidenceApproved = item.Runnable
				item.ValidationLevel = item.BaseValidationLevel
				if item.NetworkEvidenceApproved {
					item.ValidationLevel = "network"
				}
			}
			info = append(info, item)
		}
		sort.Slice(info, func(i, j int) bool { return info[i].Name < info[j].Name })
		return Reply{OK: true, Code: "ok", Enabled: c.enabled, Scenarios: info}
	case Enable:
		if !*request.Enabled && c.runID != "" && grant.Owner != c.owner && !grant.ManageAll {
			return c.failure("run_not_found")
		}
		if err := c.kernel.Enable(c.kernelContext(ctx, "enable", grant), *request.Enabled); err != nil {
			return c.failure("enable_rejected")
		}
		c.enabled = *request.Enabled
		if !c.enabled && c.runCleanup != nil {
			c.runCleanup()
			c.runCleanup = nil
		}
		return Reply{OK: true, Code: "ok", Enabled: c.enabled}
	case Start:
		if !c.enabled {
			return c.failure("disabled")
		}
		info, exists := c.catalog[request.Scenario]
		if !exists {
			return c.failure("scenario_not_registered")
		}
		if registered, ok := c.network[request.Scenario]; ok {
			info.Runnable = networkReady(registered)
			info.NetworkEvidenceApproved = info.Runnable
			info.ValidationLevel = info.BaseValidationLevel
			if info.NetworkEvidenceApproved {
				info.ValidationLevel = "network"
			}
		}
		if !info.Runnable {
			return c.failure("unsupported")
		}
		if info.ExactConcurrency != 0 && request.Concurrency != info.ExactConcurrency || info.ExactCalls != 0 && request.Calls != info.ExactCalls {
			return c.failure("scenario_budget_mismatch")
		}
		if !grant.IsolationApproved {
			return c.failure("isolated_adapter_required")
		}
		// Bind admitted task lifetime to the authenticated MANAGEMENT session and
		// module, not a short HTTP request that ends immediately after Start replies.
		runctx, cancel := context.WithCancel(c.life)
		stopSession := context.AfterFunc(grant.Lifetime, cancel)
		stopRequest := context.AfterFunc(ctx, cancel)
		id, err := c.kernel.Start(c.kernelContext(runctx, "start", grant), devtest.Plan{Scenario: request.Scenario, Concurrency: request.Concurrency, Rate: request.Rate, Calls: request.Calls, Duration: time.Duration(request.DurationMS) * time.Millisecond})
		stopRequest()
		if err != nil {
			stopSession()
			cancel()
			if errors.Is(err, devtest.ErrBusy) {
				return c.failure("busy")
			}
			return c.failure("start_rejected")
		}
		if c.runCleanup != nil {
			c.runCleanup()
		}
		c.runCleanup = func() { stopSession(); cancel() }
		c.runID = id
		c.owner = grant.Owner
		c.plannedCalls = request.Calls
		c.runInfo = info
		if ctx.Err() != nil {
			cancel()
			return Reply{Code: "request_cancelled", Enabled: c.enabled, RunID: id}
		}
		return Reply{OK: true, Code: "started", Enabled: c.enabled, RunID: id}
	case Status, Report, Cancel:
		if c.runID == "" {
			if request.Op == Status {
				return Reply{OK: true, Code: "ok", Enabled: c.enabled, Report: &RunReport{State: "idle"}}
			}
			return c.failure("run_not_found")
		}
		if request.RunID != "" && request.RunID != c.runID || grant.Owner != c.owner && !grant.ManageAll {
			return c.failure("run_not_found")
		}
		if request.Op == Cancel {
			if err := c.kernel.Cancel(c.kernelContext(ctx, "cancel", grant), c.runID); err != nil {
				return c.failure("cancel_rejected")
			}
		}
		raw, err := c.kernel.Status(c.kernelContext(ctx, "status", grant))
		if err != nil {
			return c.failure("status_rejected")
		}
		view := c.project(raw)
		if raw.State == "finished" && c.runCleanup != nil {
			c.runCleanup()
			c.runCleanup = nil
		}
		return Reply{OK: true, Code: "ok", Enabled: c.enabled, RunID: c.runID, Report: &view}
	}
	return c.failure("invalid_request")
}
func (c *Controller) project(r devtest.Report) RunReport {
	info := c.catalog[r.Scenario]
	if r.ID == c.runID && c.runID != "" {
		info = c.runInfo
	}
	state := r.State
	switch state {
	case "idle", "running", "stopping", "finished":
	default:
		state = "unknown"
	}
	reason := r.StopReason
	switch reason {
	case "", "completed", "deadline", "cancelled", "prepare_failed", "unsupported":
	default:
		reason = "unknown"
	}
	outcome := r.Outcome
	switch outcome {
	case "", "passed", "failed", "unsupported", "cancelled", "deadline", "incomplete":
	default:
		outcome = "unknown"
	}
	passed := state == "finished" && outcome == "passed" && reason == "completed" && r.Attempts > 0 && r.Attempts == c.plannedCalls && r.Passed == r.Attempts && r.Failed == 0 && !r.CleanupFailed && !r.CleanupPending
	if r.Accounts != info.Accounts || r.AuthMode != info.AuthMode || r.Profile != info.Profile {
		passed = false
		outcome = "incomplete"
	}
	if outcome == "passed" && !passed {
		outcome = "incomplete"
	}
	out := RunReport{ID: r.ID, Scenario: info.Name, Layer: info.Layer, ValidationLevel: info.ValidationLevel, Accounts: info.Accounts, AuthMode: info.AuthMode, Profile: info.Profile, NetworkEvidenceApproved: info.NetworkEvidenceApproved, State: state, StopReason: reason, Outcome: outcome, OverallPassed: passed, Attempts: r.Attempts, Passed: r.Passed, Failed: r.Failed, PeakConcurrency: r.PeakConcurrency, P50US: r.P50.Microseconds(), P95US: r.P95.Microseconds(), P99US: r.P99.Microseconds(), P50NS: r.P50.Nanoseconds(), P95NS: r.P95.Nanoseconds(), P99NS: r.P99.Nanoseconds(), CleanupFailed: r.CleanupFailed, CleanupPending: r.CleanupPending}
	if !r.Started.IsZero() {
		out.StartedMS = r.Started.UnixMilli()
	}
	if !r.Finished.IsZero() {
		out.FinishedMS = r.Finished.UnixMilli()
	}
	return out
}
func (c *Controller) Shutdown(ctx context.Context) Reply {
	if c == nil {
		return Reply{OK: true, Code: "closed"}
	}
	c.mu.Lock()
	c.closed = true
	c.enabled = false
	c.closeLife()
	if c.runCleanup != nil {
		c.runCleanup()
		c.runCleanup = nil
	}
	c.mu.Unlock()
	if err := c.kernel.Shutdown(ctx); err != nil {
		return Reply{Code: "shutdown_pending", Report: &RunReport{State: "stopping", Outcome: "incomplete", CleanupPending: true}}
	}
	return Reply{OK: true, Code: "closed"}
}
func (c *Controller) Close() { _ = c.Shutdown(context.Background()) }

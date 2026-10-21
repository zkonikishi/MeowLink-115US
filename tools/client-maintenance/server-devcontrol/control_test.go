package devcontrol

import (
	"context"
	"encoding/json"
	"errors"
	devtest "meowlink/devtest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

type proof struct {
	issuer, owner byte
	manage        bool
	kind          PrincipalKind
}

func (*proof) MeowScopeControlOpaque() {}

type auth struct {
	life      context.Context
	deny      Operation
	isolation bool
}

func (a *auth) Authorize(_ context.Context, op Operation, p HostProof) (HostGrant, error) {
	v, ok := p.(*proof)
	if !ok || v == nil || v.issuer != 42 || v.owner == 0 || op == a.deny {
		return HostGrant{}, errors.New("private-ticket-DO-NOT-LOG")
	}
	kind := v.kind
	if kind == 0 {
		kind = GM
	}
	return HostGrant{Owner: [32]byte{v.owner}, Kind: kind, Lifetime: a.life, IsolationApproved: a.isolation, ManageAll: v.manage}, nil
}

type fixture struct {
	calls  atomic.Int32
	closed atomic.Bool
	call   func(context.Context) error
	close  func() error
}

func (f *fixture) Call(ctx context.Context) error {
	f.calls.Add(1)
	if f.call != nil {
		return f.call(ctx)
	}
	return nil
}
func (f *fixture) Close() error {
	f.closed.Store(true)
	if f.close != nil {
		return f.close()
	}
	return nil
}
func scenario(name, layer string, f *fixture) ScenarioRegistration {
	return ScenarioRegistration{Scenario: devtest.Scenario{Name: name, Layer: layer, Isolated: true, Prepare: func(context.Context) (devtest.Fixture, error) { return f, nil }}}
}
func setup(t *testing.T, f *fixture) (*Controller, *auth, context.CancelFunc) {
	t.Helper()
	life, stop := context.WithCancel(context.Background())
	a := &auth{life: life, isolation: true}
	c, e := New(context.Background(), a, []ScenarioRegistration{scenario("meowlink.party.4", "domain", f)})
	if e != nil {
		t.Fatal(e)
	}
	t.Cleanup(func() { stop(); c.Close() })
	return c, a, stop
}
func allow() HostProof              { return &proof{issuer: 42, owner: 1} }
func switchRequest(on bool) Request { return Request{Op: Enable, Enabled: &on} }
func plan() Request {
	return Request{Op: Start, Scenario: "meowlink.party.4", Concurrency: 2, Rate: 100, Calls: 2, DurationMS: 1000}
}
func enable(t *testing.T, c *Controller) {
	t.Helper()
	if r := c.Handle(context.Background(), allow(), switchRequest(true)); !r.OK {
		t.Fatal(r)
	}
}
func finished(t *testing.T, c *Controller, p HostProof, id string) Reply {
	t.Helper()
	end := time.Now().Add(time.Second)
	for time.Now().Before(end) {
		r := c.Handle(context.Background(), p, Request{Op: Report, RunID: id})
		if r.Report != nil && r.Report.State == "finished" {
			return r
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("not finished")
	return Reply{}
}
func TestDecodeExactKeysAndNoArbitraryExecution(t *testing.T) {
	invalid := []string{`{"op":"list","actor":"admin"}`, `{"op":"list","url":"https://target"}`, `{"op":"list","sql":"delete"}`, `{"op":"list","script":"run"}`, `{"op":"list","op":"start"}`, `{"op":"list","Op":"list"}`, `{"OP":"list"}`, `{"op":"enable","enabled":true,"enabled":false}`, `{"op":"list","o\u0070":"list"}`, `{"op":"list"} {}`, `[]`, strings.Repeat(" ", 4097)}
	invalid = append(invalid, `{"op":"list","scenario":null}`, `{"op":"list","scenario":""}`, `{"op":"list","calls":0}`)
	for _, raw := range invalid {
		if _, e := DecodeRequest(strings.NewReader(raw)); e == nil {
			t.Fatal("accepted invalid JSON control", raw)
		}
	}
	r, e := DecodeRequest(strings.NewReader(`{"op":"start","scenario":"meowlink.party.4","concurrency":4,"rate":100,"calls":1000,"duration_ms":30000}`))
	if e != nil || r.Calls != 1000 {
		t.Fatal(e)
	}
}
func TestDefaultDisabledTrustedAuthorizationEveryOperation(t *testing.T) {
	f := &fixture{}
	c, a, _ := setup(t, f)
	if r := c.Handle(context.Background(), allow(), plan()); r.Code != "disabled" {
		t.Fatal(r)
	}
	for _, request := range []Request{{Op: List}, switchRequest(true), plan(), {Op: Status}, {Op: Cancel, RunID: "run-1"}, {Op: Report, RunID: "run-1"}} {
		var typed *proof
		for _, p := range []HostProof{nil, typed, &proof{issuer: 1, owner: 1}} {
			r := c.Handle(context.Background(), p, request)
			if r.Code != "denied" {
				t.Fatal(r)
			}
		}
	}
	a.deny = List
	r := c.Handle(context.Background(), allow(), Request{Op: List})
	b, _ := json.Marshal(r)
	if r.Code != "denied" || strings.Contains(string(b), "DO-NOT-LOG") {
		t.Fatal(string(b))
	}
	if f.calls.Load() != 0 {
		t.Fatal("unauthorized execution")
	}
}
func TestActualExecutorStartStatusReportAndHTTPLifetime(t *testing.T) {
	f := &fixture{}
	c, _, _ := setup(t, f)
	enable(t, c)
	req, cancel := context.WithCancel(context.Background())
	r := c.Handle(req, allow(), plan())
	if !r.OK {
		t.Fatal(r)
	}
	cancel()
	done := finished(t, c, allow(), r.RunID)
	if !done.Report.OverallPassed || done.Report.Outcome != "passed" || done.Report.Attempts != 2 || done.Report.ValidationLevel != "fixture" || done.Report.NativeGameplayVerified || !f.closed.Load() {
		t.Fatal(done)
	}
}
func TestOwnerIsolationAndDisableCannotBypassCancel(t *testing.T) {
	f := &fixture{call: func(ctx context.Context) error { <-ctx.Done(); return ctx.Err() }}
	c, _, _ := setup(t, f)
	enable(t, c)
	r := c.Handle(context.Background(), allow(), plan())
	if !r.OK {
		t.Fatal(r)
	}
	other := &proof{issuer: 42, owner: 2}
	for _, q := range []Request{{Op: Status}, {Op: Report, RunID: r.RunID}, {Op: Cancel, RunID: r.RunID}, switchRequest(false)} {
		if out := c.Handle(context.Background(), other, q); out.Code != "run_not_found" {
			t.Fatal(out)
		}
	}
	manager := &proof{issuer: 42, owner: 2, manage: true}
	if out := c.Handle(context.Background(), manager, Request{Op: Cancel, RunID: r.RunID}); !out.OK {
		t.Fatal(out)
	}
	done := finished(t, c, allow(), r.RunID)
	if done.Report.OverallPassed {
		t.Fatal("cancelled passed")
	}
}
func TestManagementSessionRevocationCancelsJob(t *testing.T) {
	f := &fixture{call: func(ctx context.Context) error { <-ctx.Done(); return ctx.Err() }}
	c, _, revoke := setup(t, f)
	enable(t, c)
	r := c.Handle(context.Background(), allow(), plan())
	if !r.OK {
		t.Fatal(r)
	}
	revoke()
	end := time.Now().Add(time.Second)
	for !f.closed.Load() && time.Now().Before(end) {
		time.Sleep(time.Millisecond)
	}
	if !f.closed.Load() || c.Handle(context.Background(), allow(), Request{Op: Status}).Code != "denied" {
		t.Fatal("revoked management session stayed active")
	}
}
func TestLimitsAndIsolationBeforePrepare(t *testing.T) {
	f := &fixture{}
	c, a, _ := setup(t, f)
	enable(t, c)
	for _, change := range []func(*Request){func(r *Request) { r.Concurrency = 5 }, func(r *Request) { r.Rate = 101 }, func(r *Request) { r.Calls = 1001 }, func(r *Request) { r.DurationMS = 30001 }, func(r *Request) { r.Scenario = "https://arbitrary" }} {
		r := plan()
		change(&r)
		if out := c.Handle(context.Background(), allow(), r); out.Code != "invalid_request" {
			t.Fatal(out)
		}
	}
	a.isolation = false
	if out := c.Handle(context.Background(), allow(), plan()); out.Code != "isolated_adapter_required" || f.calls.Load() != 0 {
		t.Fatal(out)
	}
}

type netAdapter struct {
	cap      NetworkCapability
	fixture  *fixture
	prepared atomic.Int32
}

func (n *netAdapter) Capability() NetworkCapability { return n.cap }
func (n *netAdapter) Prepare(context.Context) (devtest.Fixture, error) {
	n.prepared.Add(1)
	return n.fixture, nil
}
func TestNetworkRequiresApprovedServerFactoryProfileTargetAndRecheck(t *testing.T) {
	life, cancel := context.WithCancel(context.Background())
	defer cancel()
	a := &auth{life: life, isolation: true}
	profile, target := [32]byte{1}, [32]byte{2}
	adapter := &netAdapter{cap: NetworkCapability{Verified: true, Isolated: true, ProfileSHA256: profile, TargetSHA256: target}, fixture: &fixture{}}
	unsupported := scenario("meowlink.native-party.4", "transport", &fixture{})
	supported := scenario("meowlink.auth.tls.4", "transport", &fixture{})
	supported.Network = adapter
	supported.ExpectedProfileSHA256 = profile
	supported.ExpectedTargetSHA256 = target
	c, e := New(context.Background(), a, []ScenarioRegistration{unsupported, supported})
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	enable(t, c)
	listed := c.Handle(context.Background(), allow(), Request{Op: List})
	if !listed.OK || len(listed.Scenarios) != 2 || !listed.Scenarios[0].Runnable || listed.Scenarios[1].Runnable {
		t.Fatal(listed)
	}
	p := plan()
	p.Scenario = "meowlink.native-party.4"
	if out := c.Handle(context.Background(), allow(), p); out.Code != "unsupported" {
		t.Fatal(out)
	}
	p.Scenario = "meowlink.auth.tls.4"
	adapter.cap.Verified = false
	if out := c.Handle(context.Background(), allow(), p); out.Code != "unsupported" || adapter.prepared.Load() != 0 {
		t.Fatal(out)
	}
	adapter.cap.Verified = true
	out := c.Handle(context.Background(), allow(), p)
	if !out.OK {
		t.Fatal(out)
	}
	done := finished(t, c, allow(), out.RunID)
	if adapter.prepared.Load() != 1 || done.Report.ValidationLevel != "network" || done.Report.NativeGameplayVerified {
		t.Fatal(done)
	}
	// The fake network adapter above tests capability gating only. It opens no socket.
}
func TestLayerMappingAndNativeUIClaimRejected(t *testing.T) {
	a := &auth{life: context.Background(), isolation: true}
	for _, level := range []string{"native_ui", "network", "inprocess"} {
		r := scenario("meowlink.party.4", "domain", &fixture{})
		r.ValidationLevel = level
		if c, e := New(context.Background(), a, []ScenarioRegistration{r}); e == nil {
			c.Close()
			t.Fatal("layer claim accepted", level)
		}
	}
	r := scenario("host.flow", "host", &fixture{})
	c, e := New(context.Background(), a, []ScenarioRegistration{r})
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	if out := c.Handle(context.Background(), allow(), Request{Op: List}); out.Scenarios[0].ValidationLevel != "inprocess" {
		t.Fatal(out)
	}
}
func TestPrepareUnsupportedCleanupFailedAndZeroAttemptsNeverPass(t *testing.T) {
	a := &auth{life: context.Background(), isolation: true}
	for _, err := range []error{devtest.ErrUnsupported, errors.New("secret-provider-error")} {
		r := scenario("fixture.failure", "domain", &fixture{})
		r.Scenario.Prepare = func(context.Context) (devtest.Fixture, error) { return nil, err }
		c, e := New(context.Background(), a, []ScenarioRegistration{r})
		if e != nil {
			t.Fatal(e)
		}
		enable(t, c)
		p := plan()
		p.Scenario = r.Scenario.Name
		run := c.Handle(context.Background(), allow(), p)
		done := finished(t, c, allow(), run.RunID)
		if done.Report.OverallPassed || done.Report.Outcome == "passed" || done.Report.Attempts != 0 {
			t.Fatal(done)
		}
		raw, _ := json.Marshal(done)
		if strings.Contains(string(raw), "secret-provider") {
			t.Fatal("provider error leak")
		}
		c.Close()
	}
	f := &fixture{close: func() error { return errors.New("secret-cleanup") }}
	c, _, _ := setup(t, f)
	enable(t, c)
	r := c.Handle(context.Background(), allow(), plan())
	done := finished(t, c, allow(), r.RunID)
	if done.Report.OverallPassed || !done.Report.CleanupFailed {
		t.Fatal(done)
	}
}
func TestProjectionDoesNotTurnFinishedIntoPassed(t *testing.T) {
	c, _, _ := setup(t, &fixture{})
	c.plannedCalls = 2
	base := devtest.Report{ID: "run-1", Scenario: "meowlink.party.4", State: "finished", StopReason: "completed", Outcome: "passed", Attempts: 2, Passed: 2}
	if !c.project(base).OverallPassed {
		t.Fatal("valid full outcome rejected")
	}
	for _, change := range []func(*devtest.Report){func(r *devtest.Report) { r.Attempts = 0; r.Passed = 0 }, func(r *devtest.Report) { r.CleanupPending = true }, func(r *devtest.Report) { r.StopReason = "prepare_failed" }, func(r *devtest.Report) { r.StopReason = "deadline" }, func(r *devtest.Report) { r.Passed = 1 }, func(r *devtest.Report) { r.CleanupFailed = true }} {
		r := base
		change(&r)
		if c.project(r).OverallPassed || c.project(r).Outcome == "passed" {
			t.Fatal("partial outcome passed")
		}
	}
}
func TestKernelPrivateGrantCannotBeForgedFromOrdinaryContext(t *testing.T) {
	c, _, _ := setup(t, &fixture{})
	if _, e := c.kernel.List(context.Background()); e == nil {
		t.Fatal("kernel auth bypass")
	}
	if _, e := New(context.Background(), (*auth)(nil), nil); e == nil {
		t.Fatal("typed nil authorizer accepted")
	}
}
func TestBoundedShutdownDoesNotClaimStuckFixtureKilled(t *testing.T) {
	entered, block := make(chan struct{}), make(chan struct{})
	defer close(block)
	f := &fixture{call: func(context.Context) error { close(entered); <-block; return nil }}
	c, _, _ := setup(t, f)
	enable(t, c)
	p := plan()
	p.Concurrency = 1
	p.Calls = 1
	r := c.Handle(context.Background(), allow(), p)
	if !r.OK {
		t.Fatal(r)
	}
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("fixture not entered")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	out := c.Shutdown(ctx)
	if out.Code != "shutdown_pending" || out.OK || out.Report == nil || !out.Report.CleanupPending || out.Report.OverallPassed {
		t.Fatal(out)
	}
}

func TestTrustedAIUsesSameControlPlaneAndPlayerKindCannotEnable(t *testing.T) {
	c, _, _ := setup(t, &fixture{})
	p := &proof{issuer: 42, owner: 1, kind: AI}
	if out := c.Handle(context.Background(), p, switchRequest(true)); !out.OK {
		t.Fatal(out)
	}
	run := c.Handle(context.Background(), p, plan())
	if !run.OK {
		t.Fatal(run)
	}
	done := finished(t, c, p, run.RunID)
	if !done.Report.OverallPassed {
		t.Fatal(done)
	}
	if out := c.Handle(context.Background(), &proof{issuer: 42, owner: 1, kind: 99}, switchRequest(false)); out.Code != "denied" {
		t.Fatal(out)
	}
}

func TestAdapterPlanConstraintsBeforeFactoryAndMetadataProjection(t *testing.T) {
	f := &fixture{}
	r := scenario("one.round", "host", f)
	r.ExactConcurrency = 4
	r.ExactCalls = 4
	r.Scenario.Accounts = 4
	r.Scenario.AuthMode = "fixture-auth"
	r.Scenario.Profile = "fixture-profile"
	c, e := New(context.Background(), &auth{life: context.Background(), isolation: true}, []ScenarioRegistration{r})
	if e != nil {
		t.Fatal(e)
	}
	defer c.Close()
	enable(t, c)
	p := plan()
	p.Scenario = "one.round"
	if out := c.Handle(context.Background(), allow(), p); out.Code != "scenario_budget_mismatch" || f.calls.Load() != 0 {
		t.Fatal(out)
	}
	p.Concurrency = 4
	p.Calls = 4
	out := c.Handle(context.Background(), allow(), p)
	if !out.OK {
		t.Fatal(out)
	}
	done := finished(t, c, allow(), out.RunID)
	if done.Report.Accounts != 4 || done.Report.AuthMode != "fixture-auth" || done.Report.Profile != "fixture-profile" || done.Report.ValidationLevel != "inprocess" {
		t.Fatal(done)
	}
	raw := devtest.Report{ID: out.RunID, Scenario: "one.round", State: "finished", StopReason: "completed", Outcome: "passed", Accounts: 4, AuthMode: "fixture-auth", Profile: "private-provider-mismatch", Attempts: 4, Passed: 4}
	projected := c.project(raw)
	if projected.OverallPassed || projected.Outcome == "passed" || projected.Profile != "fixture-profile" {
		t.Fatal(projected)
	}
}

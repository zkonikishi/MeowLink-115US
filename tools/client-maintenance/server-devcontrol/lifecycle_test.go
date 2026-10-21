package devcontrol

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
)

func TestRPCFailureClasses(t *testing.T) {
	for code, want := range map[string]int{"denied": 403, "unsupported": 501, "disabled": 409, "scenario_budget_mismatch": 400, "run_not_found": 404, "control_adapter_fault": 500, "management_session_unavailable": 503} {
		if got := ReplyStatus(Reply{Code: code}); got != want {
			t.Fatalf("%s: %d != %d", code, got, want)
		}
	}
}

func TestLifecycleDefaultUnavailableAndExactRoute(t *testing.T) {
	var l Lifecycle
	if err := l.Boot(context.Background(), nil); err != nil {
		t.Fatal(err)
	}
	if matched, _, _ := l.Dispatch(context.Background(), ManagementPath+"/other", nil); matched {
		t.Fatal("captured unrelated GM request")
	}
	matched, status, r := l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"enable","enabled":true}`))
	if !matched || status != 503 || r.Code != "management_session_unavailable" {
		t.Fatal(status, r)
	}
	l.Shutdown(context.Background())
}

func TestLifecycleTrustedSessionAndShutdown(t *testing.T) {
	life, cancel := context.WithCancel(context.Background())
	defer cancel()
	a := &auth{life: life, isolation: true}
	var l Lifecycle
	err := l.Boot(context.Background(), &ManagementHost{Authorizer: a, Resolve: func(context.Context) (HostProof, error) { return allow(), nil }, Scenarios: []ScenarioRegistration{scenario("meowlink.party.4", "domain", &fixture{})}})
	if err != nil {
		t.Fatal(err)
	}
	defer l.Shutdown(context.Background())
	_, status, r := l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"list"}`))
	if status != 200 || !r.OK || r.Enabled {
		t.Fatal(status, r)
	}
	_, status, r = l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"enable","enabled":true,"role":"admin"}`))
	if status != 400 {
		t.Fatal(status, r)
	}
	_, status, r = l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"enable","enabled":true}`))
	if status != 200 || !r.Enabled {
		t.Fatal(status, r)
	}
	l.Shutdown(context.Background())
	_, status, _ = l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"list"}`))
	if status != 503 {
		t.Fatal("dispatch after shutdown", status)
	}
}

func TestLifecycleResolverFailureDoesNotLeak(t *testing.T) {
	for _, panics := range []bool{false, true} {
		var l Lifecycle
		a := &auth{life: context.Background(), isolation: true}
		err := l.Boot(context.Background(), &ManagementHost{Authorizer: a, Resolve: func(context.Context) (HostProof, error) {
			if panics {
				panic("private credential")
			}
			return nil, errors.New("private credential")
		}})
		if err != nil {
			t.Fatal(err)
		}
		_, status, r := l.Dispatch(context.Background(), ManagementPath, json.RawMessage(`{"op":"list"}`))
		if status != 503 || r.Code != "management_session_unavailable" {
			t.Fatal(status, r)
		}
		l.Shutdown(context.Background())
	}
}

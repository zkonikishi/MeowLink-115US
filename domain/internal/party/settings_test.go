package party

import (
	"testing"
	"time"
)

func TestConfiguredPartyRetainsModeAcrossMembershipAndLeaderChange(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 5)
	b := bindTest(t, h, 502, 5)
	s := Settings{Title: "test", Mode: 27, Route: 41, Capacity: 4, TestRemaining: 255}
	p, err := h.CreateConfigured(a, s, false)
	if err != nil {
		t.Fatal(err)
	}
	if q, e := h.CreateConfigured(a, s, false); e != nil || q.ID != p.ID {
		t.Fatal(q, e)
	}
	changed := s
	changed.Mode = 30
	if _, e := h.CreateConfigured(a, changed, false); e == nil {
		t.Fatal("repeated create replaced mode")
	}
	now := time.Unix(100, 0)
	id, err := h.Invite(a, b, now)
	if err != nil {
		t.Fatal(err)
	}
	p, err = h.Respond(b, id, true, now)
	if err != nil || p.Settings != s {
		t.Fatal(p, err)
	}
	if _, e := h.CreateConfigured(b, s, true); e == nil {
		t.Fatal("member changed leader settings")
	}
	_, p, err = h.Leave(a)
	if err != nil || p.Leader != 502 || p.Seats[1] != 502 || p.Settings != s {
		t.Fatal(p, err)
	}
	p.Settings.Mode = 0
	if q, _ := h.Of(b); q.Settings != s {
		t.Fatal("snapshot mutated shared state")
	}
	if _, e := h.CreateConfigured(a, s, true); e == nil {
		t.Fatal("departed leader updated")
	}
}

func TestConfiguredPartyCapacityIsEnforced(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	b := bindTest(t, h, 502, 1)
	s := Settings{Mode: 35, Route: 74, Capacity: 1, TestRemaining: 255}
	if _, e := h.CreateConfigured(a, s, false); e != nil {
		t.Fatal(e)
	}
	if _, e := h.Invite(a, b, time.Unix(100, 0)); e == nil {
		t.Fatal("single-person capacity ignored")
	}
	if _, e := h.RequestJoin(b, a, time.Unix(100, 0)); e == nil {
		t.Fatal("single-person application accepted")
	}
}

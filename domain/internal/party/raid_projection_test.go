package party

import (
	"testing"
	"time"
)

func TestRaidProjectionTwelveActorsIntoThreeParties(t *testing.T) {
	h := NewHub()
	var gs []RaidSubparty
	for g := 0; g < 3; g++ {
		s := RaidSubparty{Group: byte(g + 1)}
		for i := 0; i < 4; i++ {
			s.Members = append(s.Members, bindTest(t, h, uint16(100+g*4+i), 9))
		}
		gs = append(gs, s)
	}
	key := uint32(gs[0].Members[0].Context[0])<<24 | 9<<16 | 1
	ps, e := h.ProjectRaidSubparties(key, 1, gs)
	if e != nil || len(ps) != 3 {
		t.Fatal(ps, e)
	}
	for _, g := range gs {
		for _, l := range g.Members {
			p, peers := h.NetworkGroup(l)
			if p.ID != ps[g.Group].ID || len(peers) != 4 || p.Raid != (RaidProjection{key, 1, g.Group}) {
				t.Fatal(p, peers)
			}
		}
	}
	ps2, e := h.ProjectRaidSubparties(key, 1, gs)
	if e != nil {
		t.Fatal(e)
	}
	for g, p := range ps {
		if ps2[g].ID != p.ID {
			t.Fatal("retry recreated parties")
		}
	}
	h.ReleaseRaidSubparties(key, 2)
	if p, _ := h.Of(gs[0].Members[0]); p == nil {
		t.Fatal("wrong epoch released group")
	}
	h.ReleaseRaidSubparties(key, 1)
	for _, g := range gs {
		for _, l := range g.Members {
			if _, ok := h.Of(l); ok {
				t.Fatal("projection not released")
			}
		}
	}
}
func TestRaidProjectionDoesNotStealOrdinaryParty(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 1, 9)
	b := bindTest(t, h, 2, 9)
	p, e := h.Create(b)
	if e != nil {
		t.Fatal(e)
	}
	key := uint32(a.Context[0])<<24 | 9<<16 | 1
	if _, e = h.ProjectRaidSubparties(key, 1, []RaidSubparty{{1, []Lease{a}}, {2, []Lease{b}}}); e == nil {
		t.Fatal("ordinary group stolen")
	}
	if _, ok := h.Of(a); ok {
		t.Fatal("partial group left behind")
	}
	if old, _ := h.Of(b); old.ID != p.ID {
		t.Fatal("ordinary party changed")
	}
}
func TestRaidProjectionBlocksExternalJoinAndStaleLease(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 1, 9)
	b := bindTest(t, h, 2, 9)
	outside := bindTest(t, h, 3, 9)
	key := uint32(a.Context[0])<<24 | 9<<16 | 1
	if _, e := h.ProjectRaidSubparties(key, 1, []RaidSubparty{{1, []Lease{a, b}}}); e != nil {
		t.Fatal(e)
	}
	if _, e := h.Invite(a, outside, time.Now()); e == nil {
		t.Fatal("projected group invited outsider")
	}
	if _, e := h.RequestJoin(outside, a, time.Now()); e == nil {
		t.Fatal("outsider applied to projection")
	}
	if _, _, e := h.Leave(a); e == nil {
		t.Fatal("ordinary leave bypassed raid owner")
	}
	p, _ := h.Of(a)
	if e := h.LeaveGroup(a, p.ID); e == nil {
		t.Fatal("ordinary prepared departure bypassed raid owner")
	}
	if _, _, e := h.LeaveRaidMember(a, key, 2); e == nil {
		t.Fatal("wrong epoch removed member")
	}
	if _, q, e := h.LeaveRaidMember(a, key, 1); e != nil || q == nil || q.Leader != b.Actor {
		t.Fatal("owned raid leave failed", q, e)
	}
	// Remaining member still belongs to the original raid projection.
	if q, ok := h.Of(b); !ok || q.Raid.Key != key {
		t.Fatal("survivor lost projection")
	}
	h.Unbind(b)
	if _, e := h.ProjectRaidSubparties(key, 1, []RaidSubparty{{1, []Lease{a, b}}}); e == nil {
		t.Fatal("stale lease accepted")
	}
}

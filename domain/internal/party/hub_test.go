package party

import (
	"sync"
	"testing"
	"time"
)

func bindTest(t *testing.T, h *Hub, actor uint16, channel byte) Lease {
	t.Helper()
	l, e := h.Bind([2]byte{0, channel}, actor, int64(actor), "local")
	if e != nil {
		t.Fatal(e)
	}
	return l
}

func TestHubInvitationMembershipAndSnapshots(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	b := bindTest(t, h, 502, 1)
	c := bindTest(t, h, 503, 1)
	now := time.Unix(100, 0)
	id, e := h.Invite(a, b, now)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = h.Respond(c, id, true, now); e == nil {
		t.Fatal("foreign responder accepted")
	}
	p, e := h.Respond(b, id, true, now)
	if e != nil || len(p.Members) != 2 || p.ID == 1 {
		t.Fatal(p, e)
	}
	if _, e = h.Respond(b, id, true, now); e == nil {
		t.Fatal("invitation replay accepted")
	}
	p.Members[0].Name = "corrupt"
	p.Leader = 999
	current, _ := h.Of(a)
	if current.Leader != a.Actor || current.Members[0].Name == "corrupt" {
		t.Fatal("mutable snapshot leaked")
	}
	id, e = h.Invite(b, c, now)
	if e == nil {
		t.Fatal("nonleader invited")
	}
	id, e = h.Invite(a, c, now)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = h.Respond(c, id, true, now); e != nil {
		t.Fatal(e)
	}
	old, p, e := h.Leave(a)
	if e != nil || p.Leader != b.Actor || len(p.Members) != 2 || p.ID != old {
		t.Fatal(p, e)
	}
	if _, ok := h.Of(a); ok {
		t.Fatal("leaver still in party")
	}
	if p.Members[0].WireID != b.Actor || p.Members[1].WireID != c.Actor {
		t.Fatal("admission order not retained")
	}
	if p.Seats != [4]uint16{0, b.Actor, c.Actor, 0} {
		t.Fatal("native seats compacted", p.Seats)
	}
}

func TestHubStaleDisconnectCannotRemoveNewSession(t *testing.T) {
	h := NewHub()
	old := bindTest(t, h, 501, 1)
	if _, e := h.Create(old); e != nil {
		t.Fatal(e)
	}
	h.Unbind(old)
	fresh := bindTest(t, h, 501, 1)
	p, e := h.Create(fresh)
	if e != nil {
		t.Fatal(e)
	}
	h.Unbind(old)
	got, ok := h.Of(fresh)
	if !ok || got.ID != p.ID {
		t.Fatal("old cleanup removed replacement")
	}
	if _, e := h.Create(old); e == nil {
		t.Fatal("old lease created party")
	}
	if _, e := h.Bind(fresh.Context, fresh.Actor, 501, "duplicate"); e == nil {
		t.Fatal("duplicate actor replaced owner")
	}
}

func TestHubExpiredDeclinedAndWrongChannelInvitations(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	b := bindTest(t, h, 502, 1)
	foreign := bindTest(t, h, 503, 2)
	now := time.Unix(100, 0)
	if _, e := h.Invite(a, foreign, now); e == nil {
		t.Fatal("cross-channel invite")
	}
	id, _ := h.Invite(a, b, now)
	if _, e := h.Respond(b, id, true, now.Add(30*time.Second)); e == nil {
		t.Fatal("expired invite")
	}
	if _, ok := h.Of(a); ok {
		t.Fatal("failed accept created empty party")
	}
	id, _ = h.Invite(a, b, now)
	if p, e := h.Respond(b, id, false, now); e != nil || p != nil {
		t.Fatal(p, e)
	}
	if _, ok := h.Of(b); ok {
		t.Fatal("decline joined party")
	}
	id, _ = h.Invite(a, b, now)
	h.Unbind(a)
	if _, e := h.Respond(b, id, true, now); e == nil {
		t.Fatal("disconnected inviter accepted")
	}
}

func TestHubConcurrentLastSlotOnlyOneWinner(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	now := time.Unix(100, 0)
	for _, actor := range []uint16{502, 503} {
		l := bindTest(t, h, actor, 1)
		id, _ := h.Invite(a, l, now)
		if _, e := h.Respond(l, id, true, now); e != nil {
			t.Fatal(e)
		}
	}
	b := bindTest(t, h, 504, 1)
	c := bindTest(t, h, 505, 1)
	ib, _ := h.Invite(a, b, now)
	ic, _ := h.Invite(a, c, now)
	errs := make(chan error, 2)
	var wg sync.WaitGroup
	for _, v := range []struct {
		l  Lease
		id uint64
	}{{b, ib}, {c, ic}} {
		wg.Add(1)
		go func(v struct {
			l  Lease
			id uint64
		}) {
			defer wg.Done()
			_, e := h.Respond(v.l, v.id, true, now)
			errs <- e
		}(v)
	}
	wg.Wait()
	close(errs)
	wins := 0
	for e := range errs {
		if e == nil {
			wins++
		}
	}
	p, _ := h.Of(a)
	if wins != 1 || len(p.Members) != 4 {
		t.Fatal(wins, p)
	}
}

func TestHubWrapKeepsLiveIDAndLeavesNoDanglingInvites(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	b := bindTest(t, h, 502, 1)
	pa, e := h.Create(a)
	if e != nil {
		t.Fatal(e)
	}
	h.nextParty = 65535
	pb, e := h.Create(b)
	if e != nil || pa.ID != 2 || pb.ID != 3 {
		t.Fatal(pa, pb, e)
	}
	h.Leave(b)
	id, e := h.Invite(a, b, time.Unix(100, 0))
	if e != nil {
		t.Fatal(e)
	}
	h.Leave(a)
	if _, e := h.Respond(b, id, true, time.Unix(101, 0)); e == nil {
		t.Fatal("left party invitation resurrected")
	}
	if _, ok := h.Of(a); ok {
		t.Fatal("leaver resurrected")
	}
}

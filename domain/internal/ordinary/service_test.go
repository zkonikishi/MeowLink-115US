package ordinary

import (
	"dfolan/internal/dungeon"
	"testing"
	"time"
)

func TestActualHubInvitationToSharedBarrierAndDisconnect(t *testing.T) {
	s := New()
	now := time.Unix(100, 0)
	a, e := s.Bind([2]byte{0, 1}, 501, 1<<40, "one")
	if e != nil {
		t.Fatal(e)
	}
	b, e := s.Bind([2]byte{0, 1}, 502, 1<<40+1, "two")
	if e != nil {
		t.Fatal(e)
	}
	id, e := s.Invite(a, b, now)
	if e != nil {
		t.Fatal(e)
	}
	p, e := s.Accept(b, id, now)
	if e != nil || len(p.Members) != 2 {
		t.Fatal(p, e)
	}
	run, e := s.Start(a, dungeon.OrdinaryPlan{Source: "synthetic-test", Map: 10, Entities: []uint32{1001}}, now)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = s.Loaded(b, run.Stamp, now); e == nil {
		t.Fatal("unwritten map accepted")
	}
	if e = s.MapsWritten(a, run.Stamp, now); e != nil {
		t.Fatal(e)
	}
	if all, e := s.Loaded(a, run.Stamp, now); e != nil || all {
		t.Fatal(all, e)
	}
	if e = s.MapsWritten(b, run.Stamp, now); e != nil {
		t.Fatal(e)
	}
	if all, e := s.Loaded(b, run.Stamp, now); e != nil || !all {
		t.Fatal(all, e)
	}
	x, e := s.Snapshot(b)
	if e != nil || x.Stamp != run.Stamp || x.Phase != dungeon.SharedRunning || x.AdmissionCount != 2 {
		t.Fatal(x, e)
	}
	s.Disconnect(a)
	x, e = s.Snapshot(b)
	if e != nil || x.Phase != dungeon.SharedAborted {
		t.Fatal("leader disconnect failed cancellation", x, e)
	}
	fresh, e := s.Bind([2]byte{0, 1}, 501, 1<<40, "one-returned")
	if e != nil || fresh.Generation == a.Generation {
		t.Fatal(fresh, e)
	}
	s.Disconnect(a)
	if _, e = s.Invite(b, fresh, now); e != nil {
		t.Fatal("old disconnect removed replacement", e)
	}
}

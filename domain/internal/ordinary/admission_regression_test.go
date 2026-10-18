package ordinary_test

import (
	"meowlink/internal/dungeon"
	"meowlink/internal/ordinary"
	"meowlink/internal/party"
	"testing"
	"time"
)

func TestThreeMemberAdmissionRegression(t *testing.T) {
	s := ordinary.New()
	now := time.Unix(100, 0)
	var ms []party.Lease
	for i := 0; i < 3; i++ {
		m, e := s.Bind([2]byte{0, 1}, uint16(501+i), int64(11+i), "review")
		if e != nil {
			t.Fatal(e)
		}
		ms = append(ms, m)
		if i > 0 {
			id, e := s.Invite(ms[0], m, now)
			if e != nil {
				t.Fatal(e)
			}
			if _, e = s.Accept(m, id, now); e != nil {
				t.Fatal(e)
			}
		}
	}
	if _, e := s.Start(ms[0], dungeon.OrdinaryPlan{Source: "synthetic", Map: 77}, now); e != nil {
		t.Fatalf("advertised 2..4 rejects 3 actual Hub members: %v", e)
	}
}

func TestLoadingDoesNotAuthorizeCombatBeforeCommit(t *testing.T) {
	now := time.Unix(100, 0)
	ms := []party.Lease{{Context: [2]byte{0, 1}, Actor: 501, Generation: 1}, {Context: [2]byte{0, 1}, Actor: 502, Generation: 2}}
	r, e := dungeon.NewSharedRun(ms[0], ms, [4]uint16{501, 502}, dungeon.OrdinaryPlan{Source: "synthetic", Map: 77, Entities: []uint32{1001}}, now, time.Minute)
	if e != nil {
		t.Fatal(e)
	}
	v, _ := r.Snapshot()
	for _, m := range ms {
		if e = r.MapsWritten(v.Stamp, m, now); e != nil {
			t.Fatal(e)
		}
		if _, e = r.Loaded(v.Stamp, m, now); e != nil {
			t.Fatal(e)
		}
	}
	if changed, e := r.Kill(v.Stamp, ms[0], 1001, now); changed || e == nil {
		t.Fatalf("combat authorized with no resource commit: changed=%v err=%v", changed, e)
	}
}

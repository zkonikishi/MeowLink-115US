package ordinary_test

import (
	"fmt"
	"meowlink/internal/dungeon"
	"meowlink/internal/ordinary"
	"meowlink/internal/party"
	"testing"
	"time"
)

// Pure domain tests: no authentication, SQL commit, transport or DFO is claimed.
func TestOrdinaryTwoToFourMembersAndIndependentTeams(t *testing.T) {
	for _, n := range []int{2, 3, 4} {
		t.Run(fmt.Sprint(n), func(t *testing.T) {
			s := ordinary.New()
			now := time.Unix(100, 0)
			var teams [][]party.Lease
			var stamps []dungeon.SharedStamp
			for group := 0; group < 2; group++ {
				var ms []party.Lease
				for i := 0; i < n; i++ {
					m, e := s.Bind([2]byte{0, 1}, uint16(501+group*10+i), int64(100+group*10+i), "fixture")
					if e != nil {
						t.Fatal(e)
					}
					ms = append(ms, m)
					if i > 0 {
						id, e := s.Invite(ms[0], m, now)
						if e != nil {
							t.Fatal(e)
						}
						if _, e = s.AcceptFrom(ms[0], m, id, now); e != nil {
							t.Fatal(e)
						}
					}
				}
				snap, e := s.Start(ms[0], dungeon.OrdinaryPlan{Source: "synthetic-fixture", Map: uint32(77 + group)}, now)
				if e != nil {
					t.Fatal(e)
				}
				if snap.AdmissionCount != n {
					t.Fatal(snap)
				}
				for i, m := range ms {
					if snap.Seats[i] != m.Actor {
						t.Fatal("seat changed", snap.Seats)
					}
					if e = s.MapsWritten(m, snap.Stamp, now); e != nil {
						t.Fatal(e)
					}
					all, e := s.Loaded(m, snap.Stamp, now)
					if e != nil || all != (i == n-1) {
						t.Fatal(all, e)
					}
				}
				ready, e := s.Snapshot(ms[0])
				if e != nil || ready.Phase != dungeon.SharedReady {
					t.Fatal("missing resource gate", ready, e)
				}
				teams = append(teams, ms)
				stamps = append(stamps, snap.Stamp)
			}
			if stamps[0].RunID == stamps[1].RunID {
				t.Fatal("run identities aliased")
			}
			if _, e := s.Loaded(teams[1][0], stamps[0], now); e == nil {
				t.Fatal("foreign room accepted")
			}
			// Trusted-host release is exercised only as a domain state transition here.
			if e := s.Release(teams[1][0], stamps[1], now); e != nil {
				t.Fatal(e)
			}
			s.Disconnect(teams[0][n-1])
			a, e := s.Snapshot(teams[0][0])
			if e != nil || a.Phase != dungeon.SharedAborted || a.AdmissionCount != n {
				t.Fatal(a, e)
			}
			b, e := s.Snapshot(teams[1][0])
			if e != nil || b.Phase != dungeon.SharedRunning || b.AdmissionCount != n {
				t.Fatal("other team disturbed", b, e)
			}
		})
	}
}

func TestFifthOnlineActorCannotJoinFullOrdinaryParty(t *testing.T) {
	s := ordinary.New()
	now := time.Unix(100, 0)
	var ms []party.Lease
	for i := 0; i < 5; i++ {
		m, e := s.Bind([2]byte{0, 1}, uint16(501+i), int64(11+i), "fixture")
		if e != nil {
			t.Fatal("online population confused with party capacity", e)
		}
		ms = append(ms, m)
	}
	for i := 1; i < 4; i++ {
		id, e := s.Invite(ms[0], ms[i], now)
		if e != nil {
			t.Fatal(e)
		}
		if _, e = s.AcceptFrom(ms[0], ms[i], id, now); e != nil {
			t.Fatal(e)
		}
	}
	if _, e := s.Invite(ms[0], ms[4], now); e == nil {
		t.Fatal("fifth member accepted")
	}
	if _, ok := s.Roster(ms[4]); ok {
		t.Fatal("failed invitation changed membership")
	}
}

func TestLeaderDeparturePreservesSlotsAndCancelsFrozenRun(t *testing.T) {
	s := ordinary.New()
	now := time.Unix(100, 0)
	var ms []party.Lease
	for i := 0; i < 4; i++ {
		m, e := s.Bind([2]byte{0, 1}, uint16(501+i), int64(11+i), "fixture")
		if e != nil {
			t.Fatal(e)
		}
		ms = append(ms, m)
		if i > 0 {
			id, e := s.Invite(ms[0], m, now)
			if e != nil {
				t.Fatal(e)
			}
			if _, e = s.AcceptFrom(ms[0], m, id, now); e != nil {
				t.Fatal(e)
			}
		}
	}
	old, e := s.Start(ms[0], dungeon.OrdinaryPlan{Source: "synthetic-fixture", Map: 77}, now)
	if e != nil {
		t.Fatal(e)
	}
	if e = s.Leave(ms[0]); e != nil {
		t.Fatal(e)
	}
	p, ok := s.Roster(ms[1])
	if !ok || p.Leader != ms[1].Actor || p.Seats[0] != 0 || p.Seats[1] != ms[1].Actor {
		t.Fatal(p)
	}
	snap, e := s.Snapshot(ms[1])
	if e != nil || snap.Phase != dungeon.SharedAborted || snap.AdmissionCount != 4 {
		t.Fatal(snap, e)
	}
	if e = s.Release(ms[0], old.Stamp, now); e == nil {
		t.Fatal("old leader released")
	}
	fresh, e := s.Start(ms[1], dungeon.OrdinaryPlan{Source: "synthetic-fixture", Map: 77}, now)
	if e != nil || fresh.AdmissionCount != 3 || fresh.Seats[0] != 0 || fresh.Stamp.RunID == old.Stamp.RunID {
		t.Fatal(fresh, e)
	}
}

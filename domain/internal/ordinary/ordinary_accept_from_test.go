package ordinary

import (
	"testing"
	"time"
)

func TestAcceptFromExactInviterCannotConsumeOtherInvitation(t *testing.T) {
	s := New()
	now := time.Unix(100, 0)
	a, _ := s.Bind([2]byte{0, 1}, 501, 11, "a")
	b, _ := s.Bind([2]byte{0, 1}, 502, 12, "b")
	other, _ := s.Bind([2]byte{0, 1}, 503, 13, "other")
	id, e := s.Invite(a, b, now)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = s.AcceptFrom(other, b, id, now); e == nil {
		t.Fatal("unrelated source authorized invite")
	}
	p, e := s.AcceptFrom(a, b, id, now)
	if e != nil || len(p.Members) != 2 {
		t.Fatal("wrong-source attempt consumed invitation", p, e)
	}
	if _, e = s.AcceptFrom(a, b, id, now); e == nil {
		t.Fatal("accepted replay")
	}
}
func TestAcceptFromRejectsReusedInviterAndExpiry(t *testing.T) {
	for _, replace := range []bool{false, true} {
		s := New()
		now := time.Unix(100, 0)
		a, _ := s.Bind([2]byte{0, 1}, 501, 11, "a")
		b, _ := s.Bind([2]byte{0, 1}, 502, 12, "b")
		id, e := s.Invite(a, b, now)
		if e != nil {
			t.Fatal(e)
		}
		if replace {
			s.Disconnect(a)
			fresh, e := s.Bind(a.Context, a.Actor, 11, "fresh")
			if e != nil {
				t.Fatal(e)
			}
			if _, e = s.AcceptFrom(fresh, b, id, now); e == nil {
				t.Fatal("replacement inherited invite")
			}
		} else {
			if _, e = s.AcceptFrom(a, b, id, now.Add(30*time.Second)); e == nil {
				t.Fatal("expired invite")
			}
		}
	}
}

package party

import (
	"reflect"
	"testing"
)

func TestRaidCampRestorePreservesSurvivorSeatsAndOtherGroups(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 1, 9)
	b := bindTest(t, h, 2, 9)
	c := bindTest(t, h, 3, 9)
	other := bindTest(t, h, 4, 9)
	key := uint32(a.Context[0])<<24 | 9<<16 | 1
	ps, err := h.ProjectRaidSubparties(key, 1, []RaidSubparty{{Group: 1, Members: []Lease{a, b, c}}, {Group: 2, Members: []Lease{other}}})
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = h.LeaveRaidMember(a, key, 1); err != nil {
		t.Fatal(err)
	}
	before, _ := h.Of(b)
	if _, err = h.RestoreRaidCampGroup(key, 1, 1, []Lease{a, b}); err == nil {
		t.Fatal("omitted survivor removed")
	}
	after, _ := h.Of(b)
	if !reflect.DeepEqual(before, after) {
		t.Fatal("failed restore changed group")
	}
	p, err := h.RestoreRaidCampGroup(key, 1, 1, []Lease{a, b, c})
	if err != nil {
		t.Fatal(err)
	}
	if p.ID != ps[1].ID || p.Leader != b.Actor || p.Seats[1] != b.Actor || p.Seats[2] != c.Actor || p.Seats[0] != a.Actor {
		t.Fatal("survivor ID/leader/seats moved", p)
	}
	q, _ := h.Of(other)
	if !reflect.DeepEqual(q, ps[2]) {
		t.Fatal("other group changed")
	}
	if _, err = h.RestoreRaidCampGroup(key, 1, 1, []Lease{a, b, c, other}); err == nil {
		t.Fatal("stole another group's member")
	}
	for _, l := range []Lease{a, b, c} {
		if _, _, err = h.LeaveRaidMember(l, key, 1); err != nil {
			t.Fatal(err)
		}
	}
	p, err = h.RestoreRaidCampGroup(key, 1, 1, []Lease{a, b, c})
	if err != nil || p.ID == ps[1].ID || p.Leader != a.Actor {
		t.Fatal("fresh camp projection failed", p, err)
	}
	q, err = h.RestoreRaidCampGroup(key, 1, 1, []Lease{a, b, c})
	if err != nil || !reflect.DeepEqual(q, p) {
		t.Fatal("camp projection replay changed state", err)
	}
}

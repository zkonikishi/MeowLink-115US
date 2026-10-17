package party

import "testing"

func TestLeaveGroupDoesNotRemoveNewPartyOrLease(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 41, 1)
	old, e := h.Create(a)
	if e != nil {
		t.Fatal(e)
	}
	if e = h.LeaveGroup(a, old.ID); e != nil {
		t.Fatal(e)
	}
	fresh, e := h.Create(a)
	if e != nil {
		t.Fatal(e)
	}
	if e = h.LeaveGroup(a, old.ID); e == nil {
		t.Fatal("old group accepted")
	}
	if p, _ := h.Of(a); p == nil || p.ID != fresh.ID {
		t.Fatal("fresh group removed")
	}
	h.Unbind(a)
	b := bindTest(t, h, 41, 1)
	p, e := h.Create(b)
	if e != nil {
		t.Fatal(e)
	}
	if e = h.LeaveGroup(a, p.ID); e == nil {
		t.Fatal("old lease accepted")
	}
	if current, _ := h.Of(b); current == nil || current.ID != p.ID {
		t.Fatal("replacement removed")
	}
}

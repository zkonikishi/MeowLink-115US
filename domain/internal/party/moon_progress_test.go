package party

import "testing"

func TestMoonProgressFencesAndDoesNotChangeMembership(t *testing.T) {
	h := NewHub()
	l := bindTest(t, h, 501, 1)
	p, e := h.CreateConfigured(l, Settings{Mode: 27, Route: 41, Capacity: 4, TestRemaining: 255}, false)
	if e != nil {
		t.Fatal(e)
	}
	publish := func(g, r uint64, state byte, d uint32) bool {
		return h.PublishMoonProgress(l, p.ID, MoonWireProgress{g, r, state, d})
	}
	for _, v := range []MoonWireProgress{{1, 1, 2, 100004136}, {1, 2, 2, 100004137}, {1, 2, 3, 100004137}, {1, 2, 5, 100004137}} {
		if !h.PublishMoonProgress(l, p.ID, v) {
			t.Fatal(v)
		}
	}
	for _, v := range []MoonWireProgress{{1, 1, 5, 100004137}, {1, 2, 2, 100004137}, {0, 3, 2, 100004136}, {2, 1, 5, 100004137}} {
		if h.PublishMoonProgress(l, p.ID, v) {
			t.Fatal("stale accepted", v)
		}
	}
	current, _ := h.Of(l)
	if current.Seats != p.Seats || current.Leader != p.Leader || len(current.Members) != len(p.Members) {
		t.Fatal("progress changed roster")
	}
	if !publish(2, 1, 2, 100004136) || !publish(2, 1, 0, 0) || publish(2, 1, 2, 100004136) {
		t.Fatal("cancel/reset fence")
	}
	if !publish(3, 1, 2, 100004136) {
		t.Fatal("new run rejected")
	}
	old := l
	h.Unbind(l)
	l = bindTest(t, h, 501, 1)
	if h.PublishMoonProgress(old, p.ID, MoonWireProgress{4, 1, 2, 100004136}) {
		t.Fatal("old connection accepted")
	}
}

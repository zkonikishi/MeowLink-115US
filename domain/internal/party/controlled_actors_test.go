package party

import "testing"

func TestControlledActorsDoNotBecomeNetworkPeers(t *testing.T) {
	h := NewHub()
	owner := bindTest(t, h, 1, 1)
	busy := bindTest(t, h, 65533, 1)
	ids, err := h.ReserveControlledActors(owner, 3)
	if err != nil || len(ids) != 3 || ids[0] != 65532 {
		t.Fatal(ids, err)
	}
	if _, err := h.Bind(owner.Context, ids[0], 77, "human"); err == nil {
		t.Fatal("reserved actor reused by human")
	}
	if len(h.ConnectedLeases()) != 2 || !h.OwnsControlledActor(owner, ids[0]) || h.OwnsControlledActor(busy, ids[0]) {
		t.Fatal("fake connection or ownership")
	}
	p, err := h.Create(owner)
	if err != nil {
		t.Fatal(err)
	}
	got, peers := h.NetworkGroup(owner)
	if got.ID != p.ID || len(peers) != 1 || len(got.Members) != 1 {
		t.Fatal("AI entered network wait set")
	}
	first := ids[0]
	ids[0] = 4
	again, _ := h.ReserveControlledActors(owner, 3)
	if again[0] != first {
		t.Fatal("mutable alias")
	}
	if _, err := h.ReserveControlledActors(owner, 2); err == nil {
		t.Fatal("resized active reservation")
	}
	h.Unbind(owner)
	next := bindTest(t, h, 1, 1)
	fresh, err := h.ReserveControlledActors(next, 3)
	if err != nil || fresh[0] != first {
		t.Fatal(fresh, err)
	}
	h.ReleaseControlledActors(owner)
	if !h.OwnsControlledActor(next, first) {
		t.Fatal("old cleanup released new lease")
	}
}

package party

import "testing"

func TestRegistryWrapKeepsExistingParty(t *testing.T) {
	r := NewRegistry()
	old, e := r.Create(2, "old")
	if e != nil {
		t.Fatal(e)
	}
	r.next = 65535
	next, e := r.Create(3, "new")
	if e != nil {
		t.Fatal(e)
	}
	if next.ID != 2 || r.byID[old.ID] != old || r.Len() != 2 {
		t.Fatal("wrap overwrote live party")
	}
}

func TestRegistryCreateIsIdempotentPerLeader(t *testing.T) {
	r := NewRegistry()
	first, e := r.Create(509, "grad")
	if e != nil {
		t.Fatal(e)
	}
	again, e := r.Create(509, "grad")
	if e != nil {
		t.Fatal(e)
	}
	if first != again || r.Len() != 1 {
		t.Fatalf("duplicate create leaked a party: %d parties", r.Len())
	}
	if len(again.Members) != 1 || again.Members[0].WireID != 509 || again.Members[0].Name != "grad" {
		t.Fatalf("members = %+v", again.Members)
	}
}

func TestRegistryAllocatesDistinctIDsAndDisbands(t *testing.T) {
	r := NewRegistry()
	a, e := r.Create(1, "a")
	if e != nil {
		t.Fatal(e)
	}
	b, e := r.Create(2, "b")
	if e != nil {
		t.Fatal(e)
	}
	if a.ID == b.ID || a.ID == 0 || b.ID == 0 {
		t.Fatalf("ids not distinct: %d %d", a.ID, b.ID)
	}
	gone, ok := r.Disband(1)
	if !ok || gone != a {
		t.Fatalf("disband = %+v %v", gone, ok)
	}
	if _, ok = r.Of(1); ok || r.Len() != 1 {
		t.Fatalf("disband left state behind")
	}
	// 解散后重建拿到新 id：与官方样本「两次创建两个 id」一致。
	c, e := r.Create(1, "a")
	if e != nil {
		t.Fatal(e)
	}
	if c.ID == a.ID {
		t.Fatalf("reused id %d", c.ID)
	}
	if _, ok = r.Disband(9); ok {
		t.Fatalf("disband of a non-member succeeded")
	}
}

func TestRegistryRejectsInvalidInput(t *testing.T) {
	r := NewRegistry()
	for _, leader := range []uint16{0, 65535} {
		if _, e := r.Create(leader, "x"); e == nil {
			t.Fatalf("leader %d accepted", leader)
		}
	}
	if _, e := r.Create(1, ""); e == nil {
		t.Fatal("empty name accepted")
	}
	var nilRegistry *Registry
	if _, e := nilRegistry.Create(1, "x"); e == nil {
		t.Fatal("nil registry accepted")
	}
	if _, ok := nilRegistry.Disband(1); ok {
		t.Fatal("nil registry disbanded")
	}
	if nilRegistry.Len() != 0 {
		t.Fatal("nil registry has parties")
	}
}

func TestRegistryReservesBootstrapID(t *testing.T) {
	r := NewRegistry()
	r.ReserveBootstrap()
	p, e := r.Create(509, "local")
	if e != nil || p.ID != 2 {
		t.Fatal(p, e)
	}
}

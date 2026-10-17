package dungeon

import (
	"context"
	"dfolan/internal/party"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func fixture(t *testing.T) (*SharedRun, []party.Lease, time.Time) {
	t.Helper()
	ms := []party.Lease{{Context: [2]byte{0, 1}, Actor: 501, Generation: 1}, {Context: [2]byte{0, 1}, Actor: 502, Generation: 2}}
	now := time.Unix(100, 0)
	r, e := NewSharedRun(ms[0], ms, [4]uint16{501, 0, 502}, OrdinaryPlan{"pvf:test", 10, []uint32{1001}}, now, time.Minute)
	if e != nil {
		t.Fatal(e)
	}
	return r, ms, now
}
func release(t *testing.T, r *SharedRun, ms []party.Lease, now time.Time) SharedStamp {
	t.Helper()
	s, _ := r.Snapshot()
	for i, m := range ms {
		if e := r.MapsWritten(s.Stamp, m, now); e != nil {
			t.Fatal(e)
		}
		all, e := r.Loaded(s.Stamp, m, now)
		if e != nil || all != (i == len(ms)-1) {
			t.Fatal(all, e)
		}
	}
	return s.Stamp
}
func TestSharedBarrierAndRoomEpoch(t *testing.T) {
	r, ms, now := fixture(t)
	s, _ := r.Snapshot()
	if _, e := r.Loaded(s.Stamp, ms[0], now); e == nil {
		t.Fatal("load before write")
	}
	if _, e := r.Kill(s.Stamp, ms[0], 1001, now); e == nil {
		t.Fatal("combat before barrier")
	}
	st := release(t, r, ms, now)
	if ok, e := r.Kill(st, ms[1], 1001, now); !ok || e != nil {
		t.Fatal(ok, e)
	}
	if e := r.MoveRoom(st, ms[0], OrdinaryPlan{"other-pvf", 11, nil}, now); e == nil {
		t.Fatal("mixed content source")
	}
	if e := r.MoveRoom(st, ms[0], OrdinaryPlan{"pvf:test", 11, []uint32{1001}}, now); e != nil {
		t.Fatal(e)
	}
	if _, e := r.Loaded(st, ms[1], now); e == nil {
		t.Fatal("old room load")
	}
	next, _ := r.Snapshot()
	if next.Stamp.Room != st.Room+1 || next.Stamp.RunID != st.RunID || next.Phase != SharedLoading || next.AdmissionCount != 2 {
		t.Fatal(next)
	}
	next.Members[0] = party.Lease{}
	fresh, _ := r.Snapshot()
	if fresh.Members[0] != ms[0] {
		t.Fatal("mutable snapshot")
	}
}
func TestSharedConcurrentKillAndDrop(t *testing.T) {
	r, ms, now := fixture(t)
	st := release(t, r, ms, now)
	var kills, claims atomic.Int32
	var wg sync.WaitGroup
	for i := 0; i < 64; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			if ok, _ := r.Kill(st, ms[i%2], 1001, now); ok {
				kills.Add(1)
			}
			if ok, _ := r.ClaimDrop(st, ms[i%2], 1001, now); ok {
				claims.Add(1)
			}
		}(i)
	}
	wg.Wait()
	if kills.Load() != 1 || claims.Load() != 1 {
		t.Fatal(kills.Load(), claims.Load())
	}
}
func TestSharedMembershipChangesCancel(t *testing.T) {
	for _, kind := range []string{"disconnect", "replacement", "leader"} {
		t.Run(kind, func(t *testing.T) {
			r, ms, now := fixture(t)
			st := release(t, r, ms, now)
			next := append([]party.Lease(nil), ms...)
			leader := ms[0]
			switch kind {
			case "disconnect":
				next = next[:1]
			case "replacement":
				next[1].Generation++
			case "leader":
				leader = ms[1]
			}
			if r.ObserveRoster(leader, next) {
				t.Fatal("changed roster accepted")
			}
			if ok, e := r.Kill(st, ms[0], 1001, now); ok || e == nil {
				t.Fatal("cancelled combat")
			}
			s, _ := r.Snapshot()
			if s.AdmissionCount != 2 || s.Phase != SharedAborted {
				t.Fatal(s)
			}
		})
	}
}
func TestSharedTimeoutAndCommit(t *testing.T) {
	r, ms, now := fixture(t)
	s, _ := r.Snapshot()
	if _, e := r.Loaded(s.Stamp, ms[0], now.Add(time.Minute)); e == nil {
		t.Fatal("timeout accepted")
	}
	if e := r.WithRoomCommit(context.Background(), s.Stamp, ms, func() error { t.Fatal("aborted transaction invoked"); return nil }); e == nil {
		t.Fatal("aborted commit")
	}
	r, ms, now = fixture(t)
	st := release(t, r, ms, now)
	sentinel := errors.New("rollback")
	if e := r.WithRoomCommit(context.Background(), st, ms, func() error { return sentinel }); !errors.Is(e, sentinel) {
		t.Fatal(e)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if e := r.WithRoomCommit(ctx, st, ms, func() error { t.Fatal("cancelled transaction invoked"); return nil }); !errors.Is(e, context.Canceled) {
		t.Fatal(e)
	}
}
func TestSharedCommitPinsRoster(t *testing.T) {
	r, ms, now := fixture(t)
	st := release(t, r, ms, now)
	entered := make(chan struct{})
	finish := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- r.WithRoomCommit(context.Background(), st, ms, func() error { close(entered); <-finish; return nil })
	}()
	<-entered
	changed := make(chan bool, 1)
	go func() { changed <- r.ObserveRoster(ms[0], ms[:1]) }()
	select {
	case <-changed:
		t.Fatal("roster changed inside transaction")
	case <-time.After(20 * time.Millisecond):
	}
	close(finish)
	if e := <-done; e != nil {
		t.Fatal(e)
	}
	if <-changed {
		t.Fatal("departure retained run")
	}
}

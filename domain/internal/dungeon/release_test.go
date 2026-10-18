package dungeon

import (
	"testing"
	"time"
)

func TestReadyDeadlineAndDuplicateLoadCannotRelease(t *testing.T) {
	r, ms, now := fixture(t)
	s, _ := r.Snapshot()
	for _, m := range ms {
		if e := r.MapsWritten(s.Stamp, m, now); e != nil {
			t.Fatal(e)
		}
		if _, e := r.Loaded(s.Stamp, m, now); e != nil {
			t.Fatal(e)
		}
	}
	if e := r.Release(s.Stamp, ms[0], now.Add(time.Minute)); e == nil {
		t.Fatal("expired ready released")
	}
	v, _ := r.Snapshot()
	if v.Phase != SharedAborted {
		t.Fatal(v)
	}
	r, ms, now = fixture(t)
	st := release(t, r, ms, now)
	if all, e := r.Loaded(st, ms[0], now); e != nil || !all {
		t.Fatal(all, e)
	}
	v, _ = r.Snapshot()
	if v.Phase != SharedRunning {
		t.Fatal("duplicate ready regressed running", v)
	}
}

package party

import (
	"testing"
	"time"
)

func takeWake(t *testing.T, c <-chan struct{}) {
	t.Helper()
	select {
	case <-c:
	default:
		t.Fatal("missing membership wake")
	}
}
func noWake(t *testing.T, c <-chan struct{}) {
	t.Helper()
	select {
	case <-c:
		t.Fatal("unexpected membership wake")
	default:
	}
}

func TestHubWakesAllMembersAndFencesOldCleanup(t *testing.T) {
	h := NewHub()
	a, b, other := bindTest(t, h, 501, 1), bindTest(t, h, 502, 1), bindTest(t, h, 503, 1)
	aw, _ := h.Watch(a)
	bw, _ := h.Watch(b)
	ow, _ := h.Watch(other)
	takeWake(t, aw)
	takeWake(t, bw)
	takeWake(t, ow)
	now := time.Unix(100, 0)
	if _, err := h.Invite(a, b, now); err != nil {
		t.Fatal(err)
	}
	if _, err := h.RespondFrom(b, a.Actor, true, now); err != nil {
		t.Fatal(err)
	}
	takeWake(t, aw)
	takeWake(t, bw)
	takeWake(t, ow) // New parties invalidate the channel directory.
	if _, ok := h.Of(other); ok {
		t.Fatal("directory observer unexpectedly joined party")
	}
	if entries := h.Directory(other); len(entries) != 1 {
		t.Fatalf("directory observer sees %d parties, want 1", len(entries))
	}
	h.Unbind(a)
	takeWake(t, bw)
	p, _ := h.Of(b)
	if p.Leader != b.Actor || len(p.Members) != 1 {
		t.Fatal(p)
	}
	fresh := bindTest(t, h, a.Actor, 1)
	fw, _ := h.Watch(fresh)
	takeWake(t, fw)
	h.Unbind(a)
	noWake(t, fw)
	noWake(t, bw)
	if _, err := h.Watch(a); err == nil {
		t.Fatal("stale watch accepted")
	}
	if !h.IsCurrent(fresh) {
		t.Fatal("old cleanup removed new actor")
	}
}

func TestHubReplyActorMustHaveExactLiveInvitation(t *testing.T) {
	h := NewHub()
	a, b, c := bindTest(t, h, 501, 1), bindTest(t, h, 502, 1), bindTest(t, h, 503, 1)
	now := time.Unix(100, 0)
	if _, e := h.RespondFrom(b, a.Actor, true, now); e == nil {
		t.Fatal("actor alone authorized join")
	}
	if _, e := h.Invite(a, b, now); e != nil {
		t.Fatal(e)
	}
	if _, e := h.RespondFrom(c, a.Actor, true, now); e == nil {
		t.Fatal("wrong target accepted")
	}
	if p, e := h.RespondFrom(b, a.Actor, false, now); e != nil || p != nil {
		t.Fatal(p, e)
	}
	if _, e := h.RespondFrom(b, a.Actor, true, now); e == nil {
		t.Fatal("decline replay accepted")
	}
	if _, e := h.Invite(a, b, now); e != nil {
		t.Fatal(e)
	}
	if _, e := h.RespondFrom(b, a.Actor, true, now.Add(30*time.Second)); e == nil {
		t.Fatal("expired reply accepted")
	}
	if _, e := h.Invite(a, b, now.Add(time.Minute)); e != nil {
		t.Fatal(e)
	}
	h.Unbind(a)
	fresh := bindTest(t, h, a.Actor, 1)
	if _, e := h.RespondFrom(b, fresh.Actor, true, now.Add(time.Minute)); e == nil {
		t.Fatal("reused actor inherited invitation")
	}
}

func TestHubSlowWatcherCoalescesWithoutBlocking(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	c, _ := h.Watch(a)
	for i := 0; i < 100; i++ {
		if _, e := h.Create(a); e != nil {
			t.Fatal(e)
		}
		if _, _, e := h.Leave(a); e != nil {
			t.Fatal(e)
		}
	}
	takeWake(t, c)
	noWake(t, c)
	if p, ok := h.Of(a); ok || p != nil {
		t.Fatal("stale snapshot after coalescing")
	}
}

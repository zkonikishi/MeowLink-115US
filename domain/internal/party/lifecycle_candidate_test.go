package party

import (
	"testing"
	"time"
)

// Domain coverage only: these leases do not substitute for host authentication.
func TestCandidateChannelSwitchRejectsOldReplyAndCleanup(t *testing.T) {
	h := NewHub()
	a := bindTest(t, h, 501, 1)
	b := bindTest(t, h, 502, 1)
	now := time.Unix(100, 0)
	if _, err := h.Invite(a, b, now); err != nil {
		t.Fatal(err)
	}
	h.Unbind(b)
	fresh, err := h.Bind([2]byte{0, 2}, b.Actor, 1<<40, "switched")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := h.RespondFrom(b, a.Actor, true, now); err == nil {
		t.Fatal("old channel reply accepted")
	}
	if _, err := h.RespondFrom(fresh, a.Actor, true, now); err == nil {
		t.Fatal("new channel inherited invitation")
	}
	h.Unbind(b)
	if !h.IsCurrent(fresh) {
		t.Fatal("stale cleanup removed switched character")
	}
	if _, err := h.Invite(a, fresh, now); err == nil {
		t.Fatal("scene actor lookup authorized cross-channel invite")
	}
	if p, ok := h.Of(fresh); ok || p != nil {
		t.Fatal("failed invite created membership")
	}
}

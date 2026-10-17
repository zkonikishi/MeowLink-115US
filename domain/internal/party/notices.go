package party

import (
	"sort"
	"time"
)

type PendingInvite struct {
	ID        uint64
	From, To  Lease
	JoinParty uint16
}
type AcceptedNotice struct {
	ID   uint64
	Peer Lease
}

// Incoming is an immutable view. It never creates authorization from an actor ID.
func (h *Hub) Incoming(l Lease, now time.Time) []PendingInvite {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil
	}
	var out []PendingInvite
	for id, v := range h.invites {
		if !now.Before(v.expires) {
			delete(h.invites, id)
			continue
		}
		if v.to == l && h.valid(v.from) {
			out = append(out, PendingInvite{ID: id, From: v.from, To: v.to, JoinParty: v.joinParty})
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out
}

// Scene changes revoke pending authorization even if the actor subsequently
// returns to the same area. Membership itself is not altered here.
func (h *Hub) CancelInvitations(l Lease) {
	h.mu.Lock()
	defer h.mu.Unlock()
	for id, v := range h.invites {
		if v.from == l || v.to == l {
			delete(h.invites, id)
			h.notify(v.to)
		}
	}
}

func (h *Hub) Accepted(l Lease) []AcceptedNotice {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil
	}
	var out []AcceptedNotice
	for id, peer := range h.accepted[l] {
		if !h.valid(peer) || h.memberParty[l] == 0 || h.memberParty[l] != h.memberParty[peer] {
			delete(h.accepted[l], id)
			continue
		}
		out = append(out, AcceptedNotice{id, peer})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out
}

// Acknowledge only events in a successfully written batch; newer events remain.
func (h *Hub) AcknowledgeAccepted(l Lease, ids []uint64) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return
	}
	for _, id := range ids {
		delete(h.accepted[l], id)
	}
}

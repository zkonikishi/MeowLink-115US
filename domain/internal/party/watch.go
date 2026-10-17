package party

import "fmt"

// Watch reports that membership, invitations, or the directory may have changed, not a queued historic
// roster. Only the owning connection reads Of and writes its own socket.
func (h *Hub) Watch(l Lease) (<-chan struct{}, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil, fmt.Errorf("stale party watcher")
	}
	c := h.watches[l]
	if c == nil {
		c = make(chan struct{}, 1)
		h.watches[l] = c
	}
	h.notify(l)
	return c, nil
}

// Caller holds mu. Notifications coalesce and never block the shared owner.
func (h *Hub) notify(l Lease) {
	if c := h.watches[l]; c != nil {
		select {
		case c <- struct{}{}:
		default:
		}
	}
}

func (h *Hub) notifyParty(p *Party) {
	for l, id := range h.memberParty {
		if id == p.ID {
			h.notify(l)
		}
	}
}

// Wake exact current sessions even after the originating party changed.
// Used by shared-scene cancellation; no historic socket references escape.
func (h *Hub) WakeMembers(members []Lease) {
	h.mu.Lock()
	defer h.mu.Unlock()
	for _, l := range members {
		if h.valid(l) {
			h.notify(l)
		}
	}
}

package party

import "fmt"

// Only the owned physical-room coordinator may reserve an empty projection.
// It releases this reservation on room retirement or reconnect expiry.
func (h *Hub) RetainRaidProjection(id uint16, projection RaidProjection, retain bool) error {
	h.mu.Lock()
	defer h.mu.Unlock()
	p := h.parties[id]
	if !retain {
		if h.retainedRaid[id] == projection {
			delete(h.retainedRaid, id)
			if p != nil && p.Raid == projection && len(p.Members) == 0 {
				delete(h.parties, id)
			}
		}
		return nil
	}
	if p == nil || projection.Key == 0 || p.Raid != projection || len(p.Members) == 0 {
		return fmt.Errorf("retained raid projection has no live owner")
	}
	if h.retainedRaid == nil {
		h.retainedRaid = map[uint16]RaidProjection{}
	}
	h.retainedRaid[id] = projection
	return nil
}

func (h *Hub) EmptyRetainedRaidProjection(id uint16, projection RaidProjection) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	p := h.parties[id]
	return p != nil && projection.Key != 0 && p.Raid == projection && h.retainedRaid[id] == projection && len(p.Members) == 0
}

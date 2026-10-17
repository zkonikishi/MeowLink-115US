package party

// Wire projection of the shared run, never a second reward/membership owner.
type MoonWireProgress struct {
	Generation, Room uint64
	State            byte
	Dungeon          uint32
}

// Only the server's selection owner calls this after its domain transition.
// Old selection/room callbacks cannot overwrite a newer run's projection.
func (h *Hub) PublishMoonProgress(l Lease, id uint16, v MoonWireProgress) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	p := h.parties[h.memberParty[l]]
	if !h.valid(l) || p == nil || p.ID != id || p.Settings.Mode != 27 || p.Settings.Route != 41 || v.Generation == 0 {
		return false
	}
	if v.State != 0 && v.State != 2 && v.State != 3 && v.State != 5 {
		return false
	}
	if v.State == 0 && v.Dungeon != 0 || v.State != 0 && v.Dungeon != 100004136 && v.Dungeon != 100004137 {
		return false
	}
	old := p.MoonProgress
	if v.Generation < old.Generation || v.Generation == old.Generation && v.Room < old.Room {
		return false
	}
	if v.Generation > old.Generation && v.State != 0 && v.State != 2 {
		return false
	}
	if v.Generation == old.Generation && v.State != 0 && v.State < old.State {
		return false
	}
	if v.Generation == old.Generation && old.Generation != 0 && old.State == 0 && v.State != 0 {
		return false
	} // cancelled generation cannot restart
	if old == v {
		return true
	}
	p.MoonProgress = v
	h.notifyParty(p)
	return true
}

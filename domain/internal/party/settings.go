package party

import "fmt"

// Settings is shared-party state, not a per-socket UI patch. Fixed arrays and
// values keep snapshots independent while membership and leader change.
type Settings struct {
	Title                 string
	Mode, Route, Capacity byte
	Info                  uint32
	Byte5                 byte
	Word6                 uint16
	Byte8                 byte
	ModeValue             uint16
	SlotFilters           [8]byte
	Field20, Selection    uint32
	Variant, Extra        byte
	TestRemaining         byte
}

func (p *Party) Capacity() int {
	if p != nil && p.Settings.Capacity > 0 {
		return int(p.Settings.Capacity)
	}
	return 4
}

// CreateConfigured installs settings under the same lock as membership.
// An update is leader-only; a repeated create cannot overwrite another mode.
func (h *Hub) CreateConfigured(l Lease, settings Settings, update bool) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	// Legion has its own counters; don't require a fabricated conquest balance.
	ispins := settings.Mode == 11 && settings.Route == 0 // native standby: no counter-extension update
	legion := ispins || settings.Mode == 38 && settings.Route == 87 || settings.Mode == 34 && settings.Route == 44 || settings.Mode == 13 && settings.Route == 26 || settings.Mode == 16 && settings.Route == 32
	if !h.valid(l) || settings.Mode == 0 || settings.Route == 0 && !ispins || settings.Capacity < 1 || settings.Capacity > 4 || (!legion && settings.TestRemaining == 0) {
		return nil, fmt.Errorf("invalid configured party")
	}
	if p := h.parties[h.memberParty[l]]; p != nil {
		if p.Leader != l.Actor {
			return nil, fmt.Errorf("only current leader may configure party")
		}
		if !update && p.Settings != settings {
			return nil, fmt.Errorf("existing party differs from create request")
		}
		if update && (p.Settings.Mode != settings.Mode || p.Settings.Route != settings.Route) {
			return nil, fmt.Errorf("leave party before changing content mode")
		}
		if len(p.Members) > int(settings.Capacity) {
			return nil, fmt.Errorf("capacity below existing membership")
		}
		p.Settings = settings
		h.notifyParty(p)
		return snapshot(p), nil
	}
	if update {
		return nil, fmt.Errorf("no party to update")
	}
	p, err := h.create(l)
	if err != nil {
		return nil, err
	}
	p.Settings = settings
	return snapshot(p), nil
}

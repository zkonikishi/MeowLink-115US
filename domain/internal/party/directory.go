package party

import (
	"fmt"
	"sort"
)

func (h *Hub) notifyDirectory(context [2]byte) {
	for l := range h.watches {
		if l.Context == context && h.valid(l) {
			h.notify(l)
		}
	}
}

// Public directory metadata does not authorize invitations or joins.
func (h *Hub) Directory(l Lease) []*Party {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil
	}
	var out []*Party
	for _, p := range h.parties {
		if p.Raid.Key != 0 || p.Settings.Mode != 0 || len(p.Members) == 0 {
			continue
		}
		leader, ok := h.online[actorKey{l.Context, p.Leader}]
		if !ok || h.memberParty[leader.lease] != p.ID {
			continue
		}
		out = append(out, snapshot(p))
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out
}

func (h *Hub) CreateOrdinary(l Lease, s Settings, update bool) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) || s.Mode != 0 || s.Route != 0 || s.Capacity < 1 || s.Capacity > 4 || s.Title == "" {
		return nil, fmt.Errorf("invalid ordinary party settings")
	}
	p := h.parties[h.memberParty[l]]
	if p != nil {
		if p.Leader != l.Actor || p.Raid.Key != 0 || p.Settings.Mode != 0 {
			return nil, fmt.Errorf("only ordinary party leader can update settings")
		}
		if len(p.Members) > int(s.Capacity) {
			return nil, fmt.Errorf("party capacity below current members")
		}
		if !update && p.Settings.Title != "" && p.Settings != s {
			return nil, fmt.Errorf("party settings differ from repeated creation")
		}
	} else {
		if update {
			return nil, fmt.Errorf("no party to update")
		}
		var err error
		p, err = h.create(l)
		if err != nil {
			return nil, err
		}
	}
	p.Settings = s
	h.notifyParty(p)
	h.notifyDirectory(l.Context)
	return snapshot(p), nil
}

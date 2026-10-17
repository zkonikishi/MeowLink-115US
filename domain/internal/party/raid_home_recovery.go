package party

import "fmt"

// Restore one empty campaign subparty after every old connection left. Other
// subparties may still be fighting; never dissolve/recreate their projections.
func (h *Hub) RestoreRaidHomeGroup(projection RaidProjection, members []Lease) (*Party, error) {
	return h.restoreRaidHomeGroup(projection, members, false)
}

// Caller has fenced the group's cancelled physical selections.
func (h *Hub) RebuildIdleRaidHomeGroup(projection RaidProjection, members []Lease) (*Party, error) {
	return h.restoreRaidHomeGroup(projection, members, true)
}

func (h *Hub) restoreRaidHomeGroup(projection RaidProjection, members []Lease, replace bool) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if projection.Key == 0 || projection.Epoch == 0 || projection.Group == 0 || len(members) == 0 || len(members) > 4 {
		return nil, fmt.Errorf("invalid raid home recovery")
	}
	seen := map[Lease]bool{}
	for _, l := range members {
		p := h.parties[h.memberParty[l]]
		if !h.valid(l) || seen[l] || uint32(l.Context[0]) != projection.Key>>24 || uint32(l.Context[1]) != (projection.Key>>16)&255 || p != nil && (!replace || p.Raid != projection) {
			return nil, fmt.Errorf("raid recovery member already owned or unavailable")
		}
		seen[l] = true
	}
	for _, p := range h.parties {
		if p.Raid == projection {
			if !replace {
				return nil, fmt.Errorf("raid recovery still has a live projection")
			}
			for l, id := range h.memberParty {
				if id == p.ID && !seen[l] {
					return nil, fmt.Errorf("raid recovery omitted projected member")
				}
			}
		}
	}
	if replace {
		for _, l := range members {
			h.leave(l)
		}
	}
	p, err := h.create(members[0])
	if err != nil {
		return nil, err
	}
	p.Raid = projection
	for i, l := range members[1:] {
		m := h.online[actorKey{l.Context, l.Actor}]
		p.Members = append(p.Members, Member{l.Actor, m.name})
		p.Seats[i+1] = l.Actor
		h.memberParty[l] = p.ID
	}
	h.notifyParty(p)
	return snapshot(p), nil
}

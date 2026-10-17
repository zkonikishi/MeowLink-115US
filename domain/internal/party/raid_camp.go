package party

import "fmt"

// Campaign-only camp operation. Caller proves there is no active physical
// selection; existing survivors retain their party ID, leader and seats.
func (h *Hub) RestoreRaidCampGroup(key uint32, epoch uint64, group byte, members []Lease) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if key == 0 || epoch == 0 || group == 0 || len(members) == 0 || len(members) > 4 {
		return nil, fmt.Errorf("invalid camp group")
	}
	projection := RaidProjection{key, epoch, group}
	seen := map[Lease]bool{}
	var p *Party
	for _, q := range h.parties {
		if q.Raid == projection {
			if p != nil {
				return nil, fmt.Errorf("multiple camp projections")
			}
			p = q
		}
	}
	for _, l := range members {
		if !h.valid(l) || seen[l] || uint32(l.Context[0]) != key>>24 || uint32(l.Context[1]) != (key>>16)&255 {
			return nil, fmt.Errorf("invalid camp member")
		}
		seen[l] = true
		if q := h.parties[h.memberParty[l]]; q != nil && q != p {
			return nil, fmt.Errorf("camp member joined another party")
		}
	}
	if p != nil {
		for l, id := range h.memberParty {
			if id == p.ID && !seen[l] {
				return nil, fmt.Errorf("camp rebuild would remove a survivor")
			}
		}
		used := 0
		for _, actor := range p.Seats {
			if actor != 0 {
				used++
			}
		}
		if used != len(p.Members) {
			return nil, fmt.Errorf("invalid camp seats")
		}
	}
	if p == nil {
		var err error
		p, err = h.create(members[0])
		if err != nil {
			return nil, err
		}
		p.Raid = projection
	}
	var free []int
	for i, actor := range p.Seats {
		if actor == 0 {
			free = append(free, i)
		}
	}
	for _, l := range members {
		if h.memberParty[l] == p.ID {
			continue
		}
		seat := free[0]
		free = free[1:]
		p.Seats[seat] = l.Actor
		p.Members = append(p.Members, Member{l.Actor, h.online[actorKey{l.Context, l.Actor}].name})
		h.memberParty[l] = p.ID
	}
	for token, v := range h.invites {
		if seen[v.from] || seen[v.to] {
			delete(h.invites, token)
		}
	}
	h.notifyParty(p)
	return snapshot(p), nil
}

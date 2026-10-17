package party

import "fmt"

type RaidProjection struct {
	Key   uint32
	Epoch uint64
	Group byte
}
type RaidSubparty struct {
	Group   byte
	Members []Lease // Admission order; first actor is the native group leader.
}

// ProjectRaidSubparties is a server-side campaign operation, never permission
// obtained from a C11 target actor. It publishes all groups atomically and
// refuses any unrelated party instead of stealing its members.
func (h *Hub) ProjectRaidSubparties(key uint32, epoch uint64, groups []RaidSubparty) (map[byte]*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if key == 0 || epoch == 0 || len(groups) == 0 || len(groups) > 10 {
		return nil, fmt.Errorf("invalid raid projection")
	}
	seenGroups := map[byte]bool{}
	seenMembers := map[Lease]bool{}
	result := map[byte]*Party{}
	existing := 0
	for _, g := range groups {
		if g.Group == 0 || seenGroups[g.Group] || len(g.Members) == 0 || len(g.Members) > 4 {
			return nil, fmt.Errorf("invalid raid subparty size/group")
		}
		seenGroups[g.Group] = true
		var p *Party
		for _, l := range g.Members {
			if !h.valid(l) || seenMembers[l] || uint32(l.Context[0]) != key>>24 || uint32(l.Context[1]) != (key>>16)&255 {
				return nil, fmt.Errorf("invalid raid projection member")
			}
			seenMembers[l] = true
			q := h.parties[h.memberParty[l]]
			if q != nil {
				if q.Raid != (RaidProjection{key, epoch, g.Group}) || (p != nil && p.ID != q.ID) {
					return nil, fmt.Errorf("raid member belongs to another party/round")
				}
				p = q
			}
		}
		if p != nil {
			if len(p.Members) != len(g.Members) || p.Leader != g.Members[0].Actor {
				return nil, fmt.Errorf("raid projection roster changed")
			}
			for i, l := range g.Members {
				if h.memberParty[l] != p.ID || p.Seats[i] != l.Actor {
					return nil, fmt.Errorf("raid projection membership changed")
				}
			}
			existing++
			result[g.Group] = snapshot(p)
		}
	}
	for _, p := range h.parties {
		if p.Raid.Key == key && p.Raid.Epoch == epoch && !seenGroups[p.Raid.Group] {
			return nil, fmt.Errorf("existing raid projection has another group")
		}
	}
	if existing != 0 {
		if existing != len(groups) {
			return nil, fmt.Errorf("partial old raid projection")
		}
		return result, nil
	}
	var made []Lease
	for _, g := range groups {
		p, e := h.create(g.Members[0])
		if e != nil {
			for _, l := range made {
				h.leave(l)
			}
			return nil, e
		}
		p.Raid = RaidProjection{key, epoch, g.Group}
		made = append(made, g.Members[0])
		for i, l := range g.Members[1:] {
			m := h.online[actorKey{l.Context, l.Actor}]
			p.Members = append(p.Members, Member{l.Actor, m.name})
			p.Seats[i+1] = l.Actor
			h.memberParty[l] = p.ID
			made = append(made, l)
		}
		result[g.Group] = snapshot(p)
		h.notifyParty(p)
	}
	for token, v := range h.invites {
		if seenMembers[v.from] || seenMembers[v.to] {
			delete(h.invites, token)
		}
	}
	return result, nil
}

// Exact owned round only. A late cancellation cannot erase a replacement
// party or a later raid's subparty.
func (h *Hub) ReleaseRaidSubparties(key uint32, epoch uint64) {
	h.mu.Lock()
	defer h.mu.Unlock()
	for l, id := range h.memberParty {
		p := h.parties[id]
		if p != nil && p.Raid.Key == key && p.Raid.Epoch == epoch {
			h.leave(l)
		}
	}
}

// Called by the raid owner after an explicit raid departure, not ordinary
// party C13. A later callback cannot remove a member from an unrelated group.
func (h *Hub) LeaveRaidMember(l Lease, key uint32, epoch uint64) (uint16, *Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return 0, nil, fmt.Errorf("stale raid departure")
	}
	p := h.parties[h.memberParty[l]]
	if p == nil {
		return 0, nil, nil
	}
	if key == 0 || epoch == 0 || p.Raid.Key != key || p.Raid.Epoch != epoch {
		return 0, nil, fmt.Errorf("raid departure does not own subparty")
	}
	id, left := h.leave(l)
	return id, left, nil
}

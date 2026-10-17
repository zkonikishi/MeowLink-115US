package party

import "fmt"

func (h *Hub) RaidProjectionMatches(id uint16, projection RaidProjection) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	p := h.parties[id]
	return p != nil && p.Raid == projection && len(p.Members) > 0
}

func (h *Hub) IsCurrentCharacter(l Lease, character int64) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.valid(l) && h.online[actorKey{l.Context, l.Actor}].character == character
}

// Reattach one authenticated original raid participant to an existing live
// projection, at its preserved physical seat. Never create a new party or
// displace another owner, and never promote the returning former leader.
func (h *Hub) AttachRaidRoomMember(projection RaidProjection, id uint16, expected []Lease, member Lease, character int64, seat byte) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	p := h.parties[id]
	empty := p != nil && len(expected) == 0 && h.retainedRaid[id] == projection
	if p == nil || p.Raid != projection || projection.Key == 0 || projection.Epoch == 0 || projection.Group == 0 || seat >= 4 || p.Seats[seat] != 0 || len(expected) == 0 && !empty || len(expected) >= 4 || len(p.Members) != len(expected) || !h.valid(member) || h.online[actorKey{member.Context, member.Actor}].character != character || h.memberParty[member] != 0 {
		return nil, fmt.Errorf("raid room projection changed")
	}
	for i, l := range expected {
		if !h.valid(l) || l.Context != member.Context || p.Members[i].WireID != l.Actor || h.memberParty[l] != id || l.Actor == member.Actor {
			return nil, fmt.Errorf("raid reentry survivor changed")
		}
	}
	if !empty && p.Leader != expected[0].Actor || uint32(member.Context[0]) != projection.Key>>24 || uint32(member.Context[1]) != (projection.Key>>16)&255 {
		return nil, fmt.Errorf("raid reentry leader/context changed")
	}
	p.Seats[seat] = member.Actor
	if empty {
		p.Leader = member.Actor
	}
	p.Members = append(p.Members, Member{member.Actor, h.online[actorKey{member.Context, member.Actor}].name})
	h.memberParty[member] = id
	for token, v := range h.invites {
		if v.from == member || v.to == member {
			delete(h.invites, token)
		}
	}
	h.notifyParty(p)
	return snapshot(p), nil
}

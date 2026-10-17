package party

import "fmt"

// ReserveControlledActors allocates client-visible identifiers, NOT human
// leases or network endpoints. The authoritative account roster still decides
// which persistent characters may occupy these IDs. All channels share this Hub.
func (h *Hub) ReserveControlledActors(owner Lease, count int) ([]uint16, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(owner) || count < 1 || count > 255 {
		return nil, fmt.Errorf("invalid controlled actor reservation")
	}
	if existing := h.controlledByOwner[owner]; len(existing) != 0 {
		if len(existing) != count {
			return nil, fmt.Errorf("release old actor namespace before resizing")
		}
		return append([]uint16(nil), existing...), nil
	}
	var ids []uint16
	// Native User construction excludes0/65534/65535. Scan once, then commit
	// atomically, without binding these identifiers as connected participants.
	for n := 65533; n > 0 && len(ids) < count; n-- {
		key := actorKey{owner.Context, uint16(n)}
		if _, online := h.online[key]; !online && h.controlledActors[key].Generation == 0 {
			ids = append(ids, uint16(n))
		}
	}
	if len(ids) != count {
		return nil, fmt.Errorf("controlled actor namespace exhausted")
	}
	if h.controlledActors == nil {
		h.controlledActors = make(map[actorKey]Lease)
		h.controlledByOwner = make(map[Lease][]uint16)
	}
	for _, id := range ids {
		h.controlledActors[actorKey{owner.Context, id}] = owner
	}
	h.controlledByOwner[owner] = ids
	return append([]uint16(nil), ids...), nil
}

func (h *Hub) OwnsControlledActor(owner Lease, actor uint16) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.valid(owner) && h.controlledActors[actorKey{owner.Context, actor}] == owner
}
func (h *Hub) releaseControlledActors(owner Lease) {
	for _, id := range h.controlledByOwner[owner] {
		key := actorKey{owner.Context, id}
		if h.controlledActors[key] == owner {
			delete(h.controlledActors, key)
		}
	}
	delete(h.controlledByOwner, owner)
}

// Release only after the owning client's corresponding native Users are gone.
// Selection disconnect also releases the namespace; stale leases cannot free
// a newly selected character's reservations.
func (h *Hub) ReleaseControlledActors(owner Lease) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.releaseControlledActors(owner)
}

// Claim under the SAME mutex used by real-character Bind, so a real login and
// an AI invitation cannot win the same persistent character concurrently.
// Account ownership is separately checked by the roster projection.
func (h *Hub) ClaimControlledCharacter(owner Lease, character int64) error {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(owner) || character <= 0 {
		return fmt.Errorf("invalid controlled character claim")
	}
	for _, p := range h.online {
		if p.character == character {
			return fmt.Errorf("character has a real selected connection")
		}
	}
	if existing := h.controlledCharacters[character]; existing.Generation != 0 && existing != owner {
		return fmt.Errorf("character claimed by another controller")
	}
	if h.controlledCharacters == nil {
		h.controlledCharacters = make(map[int64]Lease)
	}
	h.controlledCharacters[character] = owner
	return nil
}
func (h *Hub) ReleaseControlledCharacter(owner Lease, character int64) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.controlledCharacters[character] == owner {
		delete(h.controlledCharacters, character)
	}
}

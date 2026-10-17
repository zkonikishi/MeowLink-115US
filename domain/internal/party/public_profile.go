package party

// CurrentCharacter resolves only an exact live lease; actor IDs can be reused.
func (h *Hub) CurrentCharacter(l Lease) (int64, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return 0, false
	}
	return h.online[actorKey{l.Context, l.Actor}].character, true
}

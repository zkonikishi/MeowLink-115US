package party

// A detached snapshot for owner-side balance refresh after orphan recovery.
// It contains only live selected-role leases, never credentials or sockets.
func (h *Hub) ConnectedLeases() []Lease {
	h.mu.Lock()
	defer h.mu.Unlock()
	ms := make([]Lease, 0, len(h.online))
	for _, m := range h.online {
		ms = append(ms, m.lease)
	}
	return ms
}

// A coherent membership snapshot, including selection generations. No socket
// operations run under Hub.mu. Network consumers must revalidate before IO.
func (h *Hub) NetworkGroup(l Lease) (*Party, []Lease) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil, nil
	}
	p := h.parties[h.memberParty[l]]
	if p == nil {
		return nil, nil
	}
	peers := make([]Lease, 0, len(p.Members))
	for _, m := range p.Members {
		v, ok := h.online[actorKey{l.Context, m.WireID}]
		if !ok || h.memberParty[v.lease] != p.ID {
			return nil, nil
		}
		peers = append(peers, v.lease)
	}
	return snapshot(p), peers
}
func (h *Hub) NetworkChanged(l Lease) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return
	}
	if p := h.parties[h.memberParty[l]]; p != nil {
		h.notifyParty(p)
	}
}

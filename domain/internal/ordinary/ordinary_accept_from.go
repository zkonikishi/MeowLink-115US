package ordinary

import (
	"fmt"
	"meowlink/internal/party"
	"time"
)

// AcceptFrom binds the pending invitation to the exact captured inviter and
// receiver. The host must hold both authentication and full selection guards.
// A merely live unrelated inviter cannot authorize another sender's request.
func (s *Service) AcceptFrom(from, to party.Lease, id uint64, now time.Time) (*party.Party, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.hub.IsCurrent(from) || !s.hub.IsCurrent(to) {
		return nil, fmt.Errorf("stale invitation endpoint")
	}
	match := false
	for _, pending := range s.hub.Incoming(to, now) {
		if pending.ID == id && pending.From == from && pending.To == to && pending.JoinParty == 0 {
			match = true
			break
		}
	}
	if !match {
		return nil, fmt.Errorf("invitation direction or generation mismatch")
	}
	p, e := s.hub.Respond(to, id, true, now)
	if p != nil {
		s.observe(p.ID)
	}
	return p, e
}

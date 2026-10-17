package party

import (
	"fmt"
	"sync"
	"time"
)

// Lease is a server-owned character selection generation, not a client token.
// It fences callbacks from an old socket/selection, even when WireID is reused.
type Lease struct {
	Context    [2]byte
	Actor      uint16
	Generation uint64
}

type participant struct {
	lease     Lease
	character int64
	name      string
}
type invitation struct {
	from, to  Lease
	expires   time.Time
	joinParty uint16 // nonzero: from applies to to's existing party
}
type actorKey struct {
	context [2]byte
	actor   uint16
}

// Hub owns party membership across connections. It performs no IO and never
// exposes mutable internal Party pointers. Wire notices remain the caller's
// responsibility; Join is authorized only by a live, exact invitation.
type Hub struct {
	mu                    sync.Mutex
	nextLease, nextInvite uint64
	nextParty             uint16
	online                map[actorKey]participant
	memberParty           map[Lease]uint16
	parties               map[uint16]*Party
	retainedRaid          map[uint16]RaidProjection
	invites               map[uint64]invitation
	watches               map[Lease]chan struct{}
	accepted              map[Lease]map[uint64]Lease
	controlledActors      map[actorKey]Lease
	controlledByOwner     map[Lease][]uint16
	controlledCharacters  map[int64]Lease
}

func NewHub() *Hub {
	return &Hub{nextParty: 2, online: map[actorKey]participant{}, memberParty: map[Lease]uint16{}, parties: map[uint16]*Party{}, invites: map[uint64]invitation{}, watches: map[Lease]chan struct{}{}, accepted: map[Lease]map[uint64]Lease{}}
}

func snapshot(p *Party) *Party {
	if p == nil {
		return nil
	}
	r := *p
	r.Members = append([]Member(nil), p.Members...)
	return &r
}

func (h *Hub) valid(l Lease) bool {
	p, ok := h.online[actorKey{l.Context, l.Actor}]
	return ok && l.Generation != 0 && p.lease == l
}

func (h *Hub) IsCurrent(l Lease) bool {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.valid(l)
}

func (h *Hub) Bind(context [2]byte, actor uint16, character int64, name string) (Lease, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if actor == 0 || actor == 65535 || character <= 0 || name == "" {
		return Lease{}, fmt.Errorf("invalid party identity")
	}
	if h.controlledCharacters[character].Generation != 0 {
		return Lease{}, fmt.Errorf("character is controlled by an AI party")
	}
	k := actorKey{context, actor}
	if _, exists := h.online[k]; exists || h.controlledActors[k].Generation != 0 {
		return Lease{}, fmt.Errorf("party actor already bound")
	}
	if h.nextLease == ^uint64(0) {
		return Lease{}, fmt.Errorf("party generation exhausted")
	}
	h.nextLease++
	l := Lease{context, actor, h.nextLease}
	h.online[k] = participant{l, character, name}
	return l, nil
}

func (h *Hub) create(l Lease) (*Party, error) {
	if !h.valid(l) {
		return nil, fmt.Errorf("stale party session")
	}
	if id := h.memberParty[l]; id != 0 {
		return h.parties[id], nil
	}
	for n := 0; n < 65536; n++ {
		id := h.nextParty
		h.nextParty++
		// ID1 belongs to per-connection implicit solo dungeon bootstrap.
		if id < 2 || id == 65535 || h.parties[id] != nil {
			continue
		}
		m := h.online[actorKey{l.Context, l.Actor}]
		p := &Party{ID: id, Leader: l.Actor, Members: []Member{{l.Actor, m.name}}, Seats: [4]uint16{l.Actor}}
		h.parties[id] = p
		h.memberParty[l] = id
		h.notify(l)
		return p, nil
	}
	return nil, fmt.Errorf("party ids exhausted")
}

func (h *Hub) Create(l Lease) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	p, e := h.create(l)
	return snapshot(p), e
}

func (h *Hub) Of(l Lease) (*Party, bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return nil, false
	}
	p := h.parties[h.memberParty[l]]
	return snapshot(p), p != nil
}

// Invite stores authorization only. Protocol 7's37B profile is not guessed
// here, and this domain API must not be presented as an online invite flow.
func (h *Hub) Invite(from, to Lease, now time.Time) (uint64, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(from) || !h.valid(to) || from == to || from.Context != to.Context {
		return 0, fmt.Errorf("invalid party invitation endpoints")
	}
	if h.memberParty[to] != 0 {
		return 0, fmt.Errorf("target already in party")
	}
	if p := h.parties[h.memberParty[from]]; p != nil && (p.Raid.Key != 0 || p.Leader != from.Actor || len(p.Members) >= p.Capacity()) {
		return 0, fmt.Errorf("inviter cannot add member")
	}
	return h.queuePeerRequest(from, to, 0, now)
}

// Apply to the authenticated current leader; this never joins immediately.
func (h *Hub) RequestJoin(from, leader Lease, now time.Time) (uint64, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(from) || !h.valid(leader) || from == leader || from.Context != leader.Context || h.memberParty[from] != 0 {
		return 0, fmt.Errorf("invalid party application endpoints")
	}
	p := h.parties[h.memberParty[leader]]
	if p == nil || p.Raid.Key != 0 || p.Leader != leader.Actor || len(p.Members) >= p.Capacity() {
		return 0, fmt.Errorf("target party cannot accept application")
	}
	return h.queuePeerRequest(from, leader, p.ID, now)
}

// Caller holds Hub.mu. Bind direction and exact target party for confirmation.
func (h *Hub) queuePeerRequest(from, to Lease, joinParty uint16, now time.Time) (uint64, error) {
	for id, v := range h.invites {
		if !now.Before(v.expires) {
			delete(h.invites, id)
		} else if v.from == from && v.to == to {
			if v.joinParty != joinParty {
				return 0, fmt.Errorf("pending peer request changed")
			}
			return id, nil
		} else if v.to == to {
			return 0, fmt.Errorf("target has a pending invitation")
		}
	}
	if h.nextInvite == ^uint64(0) {
		return 0, fmt.Errorf("invitation ids exhausted")
	}
	h.nextInvite++
	h.invites[h.nextInvite] = invitation{from: from, to: to, expires: now.Add(30 * time.Second), joinParty: joinParty}
	h.notify(to)
	return h.nextInvite, nil
}

func (h *Hub) Respond(to Lease, id uint64, accept bool, now time.Time) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.respond(to, id, accept, now)
}

// RespondFrom correlates CMD11's inviter actor with a server-owned invitation.
// The actor alone is NOT permission to join: both live selection generations,
// the stored target and expiry are revalidated under the same owner lock.
func (h *Hub) RespondFrom(to Lease, actor uint16, accept bool, now time.Time) (*Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	from, ok := h.online[actorKey{to.Context, actor}]
	if !ok || !h.valid(to) {
		return nil, fmt.Errorf("invalid party reply endpoints")
	}
	for id, v := range h.invites {
		if v.from == from.lease && v.to == to {
			return h.respond(to, id, accept, now)
		}
	}
	return nil, fmt.Errorf("no matching party invitation")
}

func (h *Hub) respond(to Lease, id uint64, accept bool, now time.Time) (*Party, error) {
	v, ok := h.invites[id]
	if !ok || v.to != to || !h.valid(to) {
		return nil, fmt.Errorf("no matching party invitation")
	}
	delete(h.invites, id)
	if !h.valid(v.from) || !now.Before(v.expires) || v.from.Context != to.Context {
		return nil, fmt.Errorf("expired or stale invitation")
	}
	if !accept {
		return nil, nil
	}
	joining := to
	var p *Party
	if v.joinParty != 0 {
		joining = v.from
		p = h.parties[v.joinParty]
		if p == nil || p.Raid.Key != 0 || h.memberParty[to] != p.ID || p.Leader != to.Actor || len(p.Members) >= p.Capacity() {
			return nil, fmt.Errorf("application leader or target party changed")
		}
	}
	if h.memberParty[joining] != 0 {
		return nil, fmt.Errorf("target already in party")
	}
	if v.joinParty == 0 {
		p = h.parties[h.memberParty[v.from]]
		if p != nil && (p.Raid.Key != 0 || p.Leader != v.from.Actor || len(p.Members) >= p.Capacity()) {
			return nil, fmt.Errorf("party no longer accepts invitation")
		}
		var e error
		if p == nil {
			p, e = h.create(v.from)
			if e != nil {
				return nil, e
			}
		}
	}
	m := h.online[actorKey{joining.Context, joining.Actor}]
	seat := -1
	for i, actor := range p.Seats {
		if actor == 0 {
			seat = i
			break
		}
	}
	if seat < 0 {
		return nil, fmt.Errorf("party has no free native seat")
	}
	p.Seats[seat] = joining.Actor
	p.Members = append(p.Members, Member{joining.Actor, m.name})
	h.memberParty[joining] = p.ID
	if h.accepted[v.from] == nil {
		h.accepted[v.from] = map[uint64]Lease{}
	}
	for oldID, peer := range h.accepted[v.from] {
		if peer == to || !h.valid(peer) || h.memberParty[peer] != p.ID {
			delete(h.accepted[v.from], oldID)
		}
	}
	h.accepted[v.from][id] = to
	// Once joined, no other invitation to this character remains actionable.
	for otherId, other := range h.invites {
		if other.to == joining || other.from == joining {
			delete(h.invites, otherId)
		}
	}
	h.notifyParty(p)
	h.notifyDirectory(joining.Context)
	return snapshot(p), nil
}

// Leave keeps the party ID and native seats; the next member becomes leader.
// Watchers publish the new roster; shared multiplayer dungeon runs are separate.
func (h *Hub) leave(l Lease) (uint16, *Party) {
	defer h.notifyDirectory(l.Context)
	id := h.memberParty[l]
	p := h.parties[id]
	if p == nil {
		return 0, nil
	}
	delete(h.memberParty, l)
	if seat, ok := p.Seat(l.Actor); ok {
		p.Seats[seat] = 0
	}
	delete(h.accepted, l)
	h.notify(l)
	for i, m := range p.Members {
		if m.WireID == l.Actor {
			p.Members = append(p.Members[:i], p.Members[i+1:]...)
			break
		}
	}
	if len(p.Members) == 0 {
		if h.retainedRaid[id] == p.Raid && p.Raid.Key != 0 {
			p.Leader = 0
			return id, nil
		}
		delete(h.parties, id)
		return id, nil
	}
	p.Leader = p.Members[0].WireID
	h.notifyParty(p)
	return id, snapshot(p)
}

func (h *Hub) Leave(l Lease) (uint16, *Party, error) {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.leaveCurrent(l)
}

// A prepared departure must not remove the same live character from a new
// party that was joined after preparation. Check and remove under one lock.
func (h *Hub) LeaveGroup(l Lease, expected uint16) error {
	h.mu.Lock()
	defer h.mu.Unlock()
	if expected == 0 || h.memberParty[l] != expected {
		return fmt.Errorf("party departure group changed")
	}
	_, _, e := h.leaveCurrent(l)
	return e
}

func (h *Hub) leaveCurrent(l Lease) (uint16, *Party, error) {
	if !h.valid(l) {
		return 0, nil, fmt.Errorf("stale party session")
	}
	if p := h.parties[h.memberParty[l]]; p != nil && p.Raid.Key != 0 {
		return 0, nil, fmt.Errorf("raid subparty departure must use campaign ownership")
	}
	for id, v := range h.invites {
		if v.from == l || v.to == l {
			delete(h.invites, id)
		}
	}
	id, p := h.leave(l)
	return id, p, nil
}

func (h *Hub) Unbind(l Lease) (uint16, *Party) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if !h.valid(l) {
		return 0, nil
	}
	id, p := h.leave(l)
	h.releaseControlledActors(l)
	for character, owner := range h.controlledCharacters {
		if owner == l {
			delete(h.controlledCharacters, character)
		}
	}
	delete(h.online, actorKey{l.Context, l.Actor})
	delete(h.watches, l) // never close: the old writer may still be selecting
	delete(h.accepted, l)
	for id, v := range h.invites {
		if v.from == l || v.to == l {
			delete(h.invites, id)
		}
	}
	return id, p
}

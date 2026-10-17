// Package ordinary connects the existing party Hub to the ordinary shared-run
// owner. The trusted host supplies authenticated selected character bindings;
// this package neither authenticates sockets nor encodes client packets.
package ordinary

import (
	"context"
	"dfolan/internal/dungeon"
	"dfolan/internal/party"
	"fmt"
	"sync"
	"time"
)

type Service struct {
	mu   sync.Mutex
	hub  *party.Hub
	runs map[uint16]*dungeon.SharedRun
}

func New() *Service { return &Service{hub: party.NewHub(), runs: map[uint16]*dungeon.SharedRun{}} }
func (s *Service) Bind(channel [2]byte, actor uint16, character int64, name string) (party.Lease, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.hub.Bind(channel, actor, character, name)
}
func (s *Service) Invite(from, to party.Lease, now time.Time) (uint64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.hub.Invite(from, to, now)
}
func (s *Service) Accept(to party.Lease, id uint64, now time.Time) (*party.Party, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	p, e := s.hub.Respond(to, id, true, now)
	if p != nil {
		s.observe(p.ID)
	}
	return p, e
}
func (s *Service) Roster(member party.Lease) (*party.Party, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.hub.Of(member)
}
func (s *Service) Disconnect(member party.Lease) {
	s.mu.Lock()
	defer s.mu.Unlock()
	p, _ := s.hub.Of(member)
	s.hub.Unbind(member)
	if p != nil {
		s.observe(p.ID)
	}
}
func (s *Service) Leave(member party.Lease) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	p, _ := s.hub.Of(member)
	_, _, e := s.hub.Leave(member)
	if p != nil {
		s.observe(p.ID)
	}
	return e
}

// observe runs under the coordinator lock, synchronously with Hub mutations.
// No asynchronous observer is trusted to invalidate a shared run.
func (s *Service) observe(id uint16) {
	r := s.runs[id]
	if r == nil {
		return
	}
	snap, _ := r.Snapshot()
	var current []party.Lease
	var leader party.Lease
	for _, m := range snap.Members {
		if p, ok := s.hub.Of(m); ok && p.ID == id {
			current = append(current, m)
			if p.Leader == m.Actor {
				leader = m
			}
		}
	}
	r.ObserveRoster(leader, current)
}
func (s *Service) Start(leader party.Lease, plan dungeon.OrdinaryPlan, now time.Time) (dungeon.SharedSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	p, ok := s.hub.Of(leader)
	if !ok || p.Leader != leader.Actor || len(p.Members) != 2 {
		return dungeon.SharedSnapshot{}, fmt.Errorf("preview requires two current members and leader")
	}
	if r := s.runs[p.ID]; r != nil {
		v, _ := r.Snapshot()
		if v.Phase != dungeon.SharedAborted {
			return dungeon.SharedSnapshot{}, fmt.Errorf("shared run already active")
		}
	}
	_, members := s.hub.NetworkGroup(leader)
	r, e := dungeon.NewSharedRun(leader, members, p.Seats, plan, now, 45*time.Second)
	if e != nil {
		return dungeon.SharedSnapshot{}, e
	}
	s.runs[p.ID] = r
	return r.Snapshot()
}
func (s *Service) run(member party.Lease) (*dungeon.SharedRun, error) {
	p, ok := s.hub.Of(member)
	if !ok {
		return nil, fmt.Errorf("member has no party")
	}
	r := s.runs[p.ID]
	if r == nil {
		return nil, fmt.Errorf("no shared run")
	}
	s.observe(p.ID)
	return r, nil
}
func (s *Service) MapsWritten(member party.Lease, st dungeon.SharedStamp, now time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.run(member)
	if e != nil {
		return e
	}
	return r.MapsWritten(st, member, now)
}
func (s *Service) Loaded(member party.Lease, st dungeon.SharedStamp, now time.Time) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.run(member)
	if e != nil {
		return false, e
	}
	return r.Loaded(st, member, now)
}
func (s *Service) Snapshot(member party.Lease) (dungeon.SharedSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, e := s.run(member)
	if e != nil {
		return dungeon.SharedSnapshot{}, e
	}
	return r.Snapshot()
}

// WithRoomCommit holds the coordinator too, so a synchronous Hub mutation
// cannot precede commit while its run invalidation waits on run.mu. The host
// acquires all authentication and full selection guards before calling this.
// fn is SQL-only and must never reenter this Service, Hub, or SharedRun.
func (s *Service) WithRoomCommit(ctx context.Context, leader party.Lease, stamp dungeon.SharedStamp, members []party.Lease, fn func() error) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	r, err := s.run(leader)
	if err != nil {
		return err
	}
	p, ok := s.hub.Of(leader)
	if !ok || p.Leader != leader.Actor {
		return fmt.Errorf("commit requires current leader")
	}
	return r.WithRoomCommit(ctx, stamp, members, fn)
}

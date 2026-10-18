// Package dungeon implements the ordinary shared-room owner. It does not
// implement legion/conquest rules or encode native packets.
package dungeon

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"meowlink/internal/party"
	"sync"
	"time"
)

type SharedStamp struct {
	RunID string
	Room  uint64
}
type SharedPhase byte

const (
	SharedLoading SharedPhase = iota
	SharedRunning
	SharedAborted
	SharedReady
)

type SharedSnapshot struct {
	Stamp          SharedStamp
	Phase          SharedPhase
	Leader         party.Lease
	Members        []party.Lease
	Seats          [4]uint16
	Source         string
	Map            uint32
	AdmissionCount int
	Reason         string
}
type OrdinaryPlan struct {
	Source   string
	Map      uint32
	Entities []uint32
}
type SharedRun struct {
	mu                sync.Mutex
	state             SharedSnapshot
	deadline          time.Time
	timeout           time.Duration
	delivered, loaded map[party.Lease]bool
	entities          map[uint32]bool
	drops             map[uint32]party.Lease
}

// NewSharedRun takes a validated typed content plan, not a single-player Run.
// The host must authorize members under its existing account/selection guards.
// This ordinary-only API deliberately does not claim donor Instance compatibility.
func NewSharedRun(leader party.Lease, members []party.Lease, seats [4]uint16, plan OrdinaryPlan, now time.Time, timeout time.Duration) (*SharedRun, error) {
	if len(members) < 2 || len(members) > 4 || timeout <= 0 || plan.Source == "" || plan.Map == 0 {
		return nil, fmt.Errorf("invalid ordinary admission")
	}
	seen := map[party.Lease]bool{}
	actors := map[uint16]bool{}
	for _, m := range members {
		if m.Generation == 0 || m.Actor == 0 || m.Context != leader.Context || seen[m] || actors[m.Actor] {
			return nil, fmt.Errorf("invalid frozen roster")
		}
		seen[m] = true
		actors[m.Actor] = true
	}
	if !seen[leader] {
		return nil, fmt.Errorf("leader outside roster")
	}
	seatSeen := map[uint16]bool{}
	for _, a := range seats {
		if a == 0 {
			continue
		}
		if !actors[a] || seatSeen[a] {
			return nil, fmt.Errorf("invalid seats")
		}
		seatSeen[a] = true
	}
	if len(seatSeen) != len(members) {
		return nil, fmt.Errorf("incomplete seats")
	}
	entities, err := entitySet(plan.Entities)
	if err != nil {
		return nil, err
	}
	var id [16]byte
	if _, err = rand.Read(id[:]); err != nil {
		return nil, err
	}
	r := &SharedRun{state: SharedSnapshot{Stamp: SharedStamp{hex.EncodeToString(id[:]), 1}, Phase: SharedLoading, Leader: leader, Members: append([]party.Lease(nil), members...), Seats: seats, Source: plan.Source, Map: plan.Map, AdmissionCount: len(members)}, timeout: timeout, entities: entities}
	r.reset(now)
	return r, nil
}
func entitySet(ids []uint32) (map[uint32]bool, error) {
	m := map[uint32]bool{}
	for _, id := range ids {
		if id == 0 || m[id] {
			return nil, fmt.Errorf("invalid entity plan")
		}
		m[id] = true
	}
	return m, nil
}
func (r *SharedRun) reset(now time.Time) {
	r.deadline = now.Add(r.timeout)
	r.delivered = map[party.Lease]bool{}
	r.loaded = map[party.Lease]bool{}
	r.drops = map[uint32]party.Lease{}
}
func (r *SharedRun) Snapshot() (SharedSnapshot, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	s := r.state
	s.Members = append([]party.Lease(nil), s.Members...)
	return s, nil
}
func (r *SharedRun) check(st SharedStamp, m party.Lease, now time.Time) error {
	if r.state.Phase == SharedAborted || st != r.state.Stamp {
		return fmt.Errorf("stale room")
	}
	found := false
	for _, x := range r.state.Members {
		if x == m {
			found = true
		}
	}
	if !found {
		return fmt.Errorf("stale member")
	}
	if (r.state.Phase == SharedLoading || r.state.Phase == SharedReady) && !now.Before(r.deadline) {
		r.abort("loading timeout")
		return fmt.Errorf("loading timeout")
	}
	return nil
}
func (r *SharedRun) abort(reason string) { r.state.Phase = SharedAborted; r.state.Reason = reason }

// ObserveRoster cancels on departure, replacement, or leader takeover. It never
// shrinks the admission count or silently releases the remaining members.
func (r *SharedRun) ObserveRoster(leader party.Lease, members []party.Lease) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.state.Phase == SharedAborted {
		return false
	}
	same := leader == r.state.Leader && len(members) == len(r.state.Members)
	seen := map[party.Lease]bool{}
	for _, m := range members {
		if seen[m] {
			same = false
		}
		seen[m] = true
	}
	for _, m := range r.state.Members {
		if !seen[m] {
			same = false
		}
	}
	if !same {
		r.abort("membership or leader changed")
	}
	return same
}
func (r *SharedRun) MapsWritten(st SharedStamp, m party.Lease, now time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := r.check(st, m, now); e != nil {
		return e
	}
	r.delivered[m] = true
	return nil
}
func (r *SharedRun) Loaded(st SharedStamp, m party.Lease, now time.Time) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := r.check(st, m, now); e != nil {
		return false, e
	}
	if !r.delivered[m] {
		return false, fmt.Errorf("map batch not written")
	}
	if r.state.Phase == SharedReady || r.state.Phase == SharedRunning {
		return true, nil
	}
	r.loaded[m] = true
	if len(r.loaded) == r.state.AdmissionCount {
		r.state.Phase = SharedReady
		return true, nil
	}
	return false, nil
}

// Kill resolves one shared entity once; duplicate reports cannot mint drops.
// Release follows a successful host resource commit. Readiness alone cannot
// start combat. The host must retain the same selection/room guards between
// commit and this call, or retry the identical idempotent charge before release.
func (r *SharedRun) Release(st SharedStamp, leader party.Lease, now time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if err := r.check(st, leader, now); err != nil {
		return err
	}
	if leader != r.state.Leader || (r.state.Phase != SharedReady && r.state.Phase != SharedRunning) {
		return fmt.Errorf("room not ready for leader release")
	}
	r.state.Phase = SharedRunning
	return nil
}

func (r *SharedRun) Kill(st SharedStamp, m party.Lease, entity uint32, now time.Time) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := r.check(st, m, now); e != nil {
		return false, e
	}
	if r.state.Phase != SharedRunning {
		return false, fmt.Errorf("room not released")
	}
	alive, ok := r.entities[entity]
	if !ok {
		return false, fmt.Errorf("unknown entity")
	}
	if !alive {
		return false, nil
	}
	r.entities[entity] = false
	return true, nil
}

// ClaimDrop is a reservation only. Inventory persistence belongs to the host's
// single Store transaction; this result must never be treated as a DB grant.
func (r *SharedRun) ClaimDrop(st SharedStamp, m party.Lease, entity uint32, now time.Time) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := r.check(st, m, now); e != nil {
		return false, e
	}
	alive, ok := r.entities[entity]
	if r.state.Phase != SharedRunning || !ok || alive {
		return false, fmt.Errorf("drop unavailable")
	}
	if owner, ok := r.drops[entity]; ok {
		if owner == m {
			return false, nil
		}
		return false, fmt.Errorf("drop owned by another member")
	}
	r.drops[entity] = m
	return true, nil
}
func (r *SharedRun) MoveRoom(st SharedStamp, leader party.Lease, plan OrdinaryPlan, now time.Time) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := r.check(st, leader, now); e != nil {
		return e
	}
	if leader != r.state.Leader || r.state.Phase != SharedRunning || plan.Source != r.state.Source || plan.Map == 0 || r.state.Stamp.Room == ^uint64(0) {
		return fmt.Errorf("invalid room transition")
	}
	for _, alive := range r.entities {
		if alive {
			return fmt.Errorf("room not cleared")
		}
	}
	entities, e := entitySet(plan.Entities)
	if e != nil {
		return e
	}
	r.state.Stamp.Room++
	r.state.Map = plan.Map
	r.state.Phase = SharedLoading
	r.entities = entities
	r.reset(now)
	return nil
}

// WithRoomCommit pins this exact room and frozen roster across a bounded host
// transaction. Lock order: host sorted account/selection guards, then run, then
// Store. Callback must not call this owner, Hub, socket, or observer APIs.
// The host still owns authentication; this guard alone never authorizes a user.
func (r *SharedRun) WithRoomCommit(ctx context.Context, st SharedStamp, members []party.Lease, fn func() error) error {
	if fn == nil {
		return fmt.Errorf("missing transaction")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if e := ctx.Err(); e != nil {
		return e
	}
	if st != r.state.Stamp || r.state.Phase == SharedAborted || len(members) != len(r.state.Members) {
		return fmt.Errorf("stale commit roster")
	}
	seen := map[party.Lease]bool{}
	for _, m := range members {
		if seen[m] {
			return fmt.Errorf("duplicate commit member")
		}
		seen[m] = true
	}
	for _, m := range r.state.Members {
		if !seen[m] {
			return fmt.Errorf("changed commit member")
		}
	}
	return fn()
}

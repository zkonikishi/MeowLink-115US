// Package party 持有115队伍的内存状态。G0115在线owner为Hub。
//
// 范围（G0109）：只支撑「创建队伍 / 解散」这一条最小链路 —— 为客户端 op=12 / op=13 提供
// 稳定的队伍 id 与成员表。**不做**邀请、申请、匹配、踢人、跨服、持久化。
// 队伍是连接级状态：断线或进程重启即消失（官方有跨会话的队伍服务，我们没实现，
// 也不在别处假装实现了）。
package party

import "fmt"

// Member 是队伍成员。Name 来自角色表，WireID 是场景里的 actor id。
type Member struct {
	WireID uint16
	Name   string
}

// Party 是一个队伍。ID 是服务端分配的 u16（客户端在 op=9 的 [4..5] 里原样回读，
// 见 protocol/party.go 的取证说明）。
type Party struct {
	Raid         RaidProjection
	MoonProgress MoonWireProgress
	Settings     Settings
	ID           uint16
	Leader       uint16
	Members      []Member
	// Seats are stable for a membership lifetime; Members is admission order.
	// A leader change never moves surviving actors between native slots.
	Seats [4]uint16
}

func (p *Party) Seat(actor uint16) (int, bool) {
	if p != nil && actor != 0 && actor != 65535 {
		for i, a := range p.Seats {
			if a == actor {
				return i, true
			}
		}
	}
	return 0, false
}

// Registry is the legacy isolated prototype, retained for historical tests.
// Deprecated: production must use Hub (shared owner and generation fencing).
type Registry struct {
	next              uint16
	bootstrapReserved bool
	byID              map[uint16]*Party
	byLead            map[uint16]*Party
}

// ReserveBootstrap keeps the implicit solo roster's ID separate from an
// explicit party. The reservation lasts only for this character session.
func (r *Registry) ReserveBootstrap() {
	if r != nil {
		r.bootstrapReserved = true
	}
}

// NewRegistry 建一张空表。id 从 1 开始（0 留作「无队伍」）。
func NewRegistry() *Registry {
	return &Registry{next: 1, byID: map[uint16]*Party{}, byLead: map[uint16]*Party{}}
}

// Create 让 leader 建队（队长即唯一成员）并把队伍返回。
//
// 已经在队里时返回既有队伍、不另分配 id：官方样本里连续两次创建确实拿到两个不同 id，
// 但那两次中间夹了一次离开；重复创建换 id 只会让双击泄漏队伍，我们不做。
func (r *Registry) Create(leader uint16, name string) (*Party, error) {
	if r == nil {
		return nil, fmt.Errorf("party registry unavailable")
	}
	if leader == 0 || leader == 65535 {
		return nil, fmt.Errorf("invalid party leader")
	}
	if name == "" {
		return nil, fmt.Errorf("empty party leader name")
	}
	if existing, ok := r.byLead[leader]; ok {
		return existing, nil
	}
	// Never overwrite a live party on wraparound; reserve both sentinels.
	var id uint16
	for scanned := 0; scanned < 65536; scanned++ {
		candidate := r.next
		r.next++
		if candidate == 0 || candidate == 65535 || (candidate == 1 && r.bootstrapReserved) {
			continue
		}
		if _, used := r.byID[candidate]; !used {
			id = candidate
			break
		}
	}
	if id == 0 {
		return nil, fmt.Errorf("party ids exhausted")
	}
	p := &Party{ID: id, Leader: leader, Members: []Member{{WireID: leader, Name: name}}}
	r.byID[id] = p
	r.byLead[leader] = p
	return p, nil
}

// Disband 解散 leader 所在的队伍；返回被解散的队伍以及是否真的解散了。
func (r *Registry) Disband(leader uint16) (*Party, bool) {
	if r == nil {
		return nil, false
	}
	p, ok := r.byLead[leader]
	if !ok {
		return nil, false
	}
	delete(r.byLead, leader)
	delete(r.byID, p.ID)
	return p, true
}

// Of 返回 leader 所在的队伍。
func (r *Registry) Of(leader uint16) (*Party, bool) {
	if r == nil {
		return nil, false
	}
	p, ok := r.byLead[leader]
	return p, ok
}

// Len 是当前队伍数（诊断与测试用）。
func (r *Registry) Len() int {
	if r == nil {
		return 0
	}
	return len(r.byID)
}

#pragma once
#include <array>
#include <cstddef>
#include <cstdint>
#include <span>
#include <thread>

namespace us115::ai {
struct NativePartyNearbyMember { const void* user{}; uint16_t actor{}; };
struct NativePartyNearbyGate {
    std::thread::id game_thread;
    bool (*read)(void*, const void*, void*, std::size_t){};
    void* read_context{};
    void* (*party_link)(void*){};
    const void* owner_user{};
    uint16_t owner_actor{};
    std::array<NativePartyNearbyMember,255> members{};
    uint8_t member_count{};
};

// Installs only the per-member locality block in the observed selection gate.
// Native success is preserved; all other selection conditions stay native.
bool install_native_party_nearby_gate(uintptr_t entry) noexcept;
// The immutable gate is replaced only on the verified game thread. Old small
// snapshots remain allocated until process exit so the hook never races free.
bool arm_native_party_nearby_gate(const NativePartyNearbyGate&) noexcept;
void disarm_native_party_nearby_gate() noexcept;
// Exposed for isolated fixtures; performs reads only and never patches code.
bool controlled_party_nearby_override(const NativePartyNearbyGate&) noexcept;
bool controlled_party_member_nearby_override(const NativePartyNearbyGate&,const void* member_user) noexcept;
// Used only after native locality fails. A selected local AI need not own a
// town Actor or share the town scene; actual dungeon preparation is separate.
bool controlled_party_locality_override(const NativePartyNearbyGate&,const void* member_user,
    const void* member_actor,const void* owner_scene) noexcept;
bool build_native_party_selection_patch(uintptr_t entry,uintptr_t relay,std::array<std::byte,33>&) noexcept;
struct NativePartyNearbyStatus {
    uint32_t installed{},armed{},last_detail{};
    uint64_t calls{},original_accepts{},override_accepts{};
};
NativePartyNearbyStatus native_party_nearby_status() noexcept;
bool native_party_nearby_candidate_active() noexcept;
// Local peer completion and door roster filtering reuse the same exact owned
// User/party/seat validation as the locality exception.
bool install_native_local_peer_gates(uintptr_t complete,uintptr_t manager_slot,uintptr_t door_call,uintptr_t command_begin,uintptr_t map_scene_call) noexcept;
void set_native_transition_callback(void(*)(void*)) noexcept;
void maintain_native_local_peers() noexcept;
struct NativeLocalPeerStatus {
    uint32_t installed{},detail{};
    uint64_t sync_completed{},door_excluded{};
};
NativeLocalPeerStatus native_local_peer_status() noexcept;
}

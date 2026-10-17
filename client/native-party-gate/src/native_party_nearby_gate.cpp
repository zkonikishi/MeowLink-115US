#define NOMINMAX
#include <windows.h>
#include "native_party_nearby_gate.hpp"
#include "runtime_patch.hpp"
#include <atomic>
#include <cstring>
#include <limits>
#include <new>

namespace us115::ai {
namespace {
using SceneCompare=bool(__fastcall*)(void*,void*);
std::atomic<SceneCompare> original_compare{};
std::atomic<const NativePartyNearbyGate*> active{};
std::atomic_uintptr_t installed{};
std::atomic_uint64_t calls{},original_accepts{},override_accepts{};
std::atomic_uint last_detail{};
using PartyUserLookup=void*(__fastcall*)(void*,uint32_t);
using CompleteLocalPeer=void(__fastcall*)(void*,int32_t,void*);
std::atomic<PartyUserLookup> original_door_lookup{};
std::atomic<CompleteLocalPeer> complete_local_peer{};
std::atomic_uintptr_t peer_manager_slot{},door_site{};
std::atomic_uint peer_detail{};
std::atomic_uint64_t peer_completions{},door_exclusions{};
using PacketBegin=uintptr_t(__fastcall*)(void*,uint16_t);
std::atomic<PacketBegin> original_packet_begin{};
std::atomic<void(*)(void*)> transition_callback{};
thread_local bool in_transition{};
void observe_transition(void* context) noexcept {
    const auto call=transition_callback.load(std::memory_order_acquire);
    if(!call || in_transition || !native_party_nearby_candidate_active())return;
    in_transition=true;call(context);in_transition=false;
}
uintptr_t __fastcall packet_begin(void* writer,uint16_t opcode) noexcept {
    const auto original=original_packet_begin.load(std::memory_order_acquire);
    if(!original)return 0;
    // Retirement can issue native messages. Run it before the original clears
    // and begins the requested packet, never in the middle of its payload.
    if(opcode==42 || opcode==45)observe_transition(nullptr);
    return original(writer,opcode);
}
void absolute_jump(unsigned char* out,uintptr_t target) noexcept {
    const unsigned char code[]{0xff,0x25,0,0,0,0};std::memcpy(out,code,6);std::memcpy(out+6,&target,8);
}
void* packet_trampoline(uintptr_t entry,const std::array<unsigned char,15>& prologue) noexcept {
    auto* p=static_cast<unsigned char*>(VirtualAlloc(nullptr,4096,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
    if(!p)return nullptr;
    std::memcpy(p,prologue.data(),prologue.size());absolute_jump(p+15,entry+15);
    // Exact copied prologue: pushes rbp/rsi/rdi/r14/r15, allocates128 bytes.
    // Supply matching unwind data for the copied prologue and tail jump.
    const unsigned char unwind[]{1,15,6,0,15,0xf2,8,0xf0,6,0xe0,4,0x70,3,0x60,2,0x50};
    std::memcpy(p+64,unwind,sizeof(unwind));
    auto* table=reinterpret_cast<RUNTIME_FUNCTION*>(p+96);*table={0,29,64};
    DWORD prior{};
    if(!VirtualProtect(p,4096,PAGE_EXECUTE_READ,&prior) || !RtlAddFunctionTable(table,1,reinterpret_cast<DWORD64>(p))) {
        VirtualFree(p,0,MEM_RELEASE);return nullptr;
    }
    FlushInstructionCache(GetCurrentProcess(),p,29);return p;
}
constexpr std::array<unsigned char,33> prologue{
    0x4d,0x85,0xf6,0x74,0x1c,0x48,0x85,0xc0,0x0f,0x84,0xfc,0x00,0x00,0x00,
    0x49,0x8b,0xd6,0x48,0x8b,0xc8,0xe8,0x08,0x51,0x0f,0xff,0x84,0xc0,0x0f,0x85,0xe9,0x00,0x00,0x00};
template<class T> bool read(const NativePartyNearbyGate& gate,uintptr_t at,T& value) noexcept {
    return gate.read && at>=65536 && at<=std::numeric_limits<uintptr_t>::max()-sizeof(T) &&
        gate.read(gate.read_context,reinterpret_cast<const void*>(at),&value,sizeof(value));
}
uint32_t evaluate(const NativePartyNearbyGate& gate) noexcept;
void* __fastcall door_lookup(void* directory,uint32_t seat) noexcept {
    const auto lookup=original_door_lookup.load(std::memory_order_acquire);
    if(!lookup)return nullptr;
    void* user{};
    try {user=lookup(directory,seat);}catch(...){return nullptr;}
    const auto* gate=active.load(std::memory_order_acquire);
    if(seat<8 && user && gate && controlled_party_member_nearby_override(*gate,user)) {
        door_exclusions.fetch_add(1,std::memory_order_relaxed);return nullptr;
    }
    return user;
}
bool __fastcall hooked(void* member_user,void* member_actor,void* owner_scene) noexcept {
    calls.fetch_add(1,std::memory_order_relaxed);
    const auto compare=original_compare.load(std::memory_order_acquire);
    if(!compare){last_detail.store(1,std::memory_order_relaxed);return false;}
    bool accepted{};
    // Preserve the exact native successful branch. The first argument to the
    // comparator is the owner's scene, not another character Actor.
    try {accepted=member_actor && (!owner_scene || compare(owner_scene,member_actor));}
    catch(...){last_detail.store(2,std::memory_order_relaxed);return false;}
    if(accepted){original_accepts.fetch_add(1,std::memory_order_relaxed);last_detail.store(100,std::memory_order_relaxed);return true;}
    // Locally controlled AI has no remote town-arrival obligation. Its native
    // reference may be absent OR non-null; neither decides locality. Scope
    // below still requires this plugin's exact seated User and owned party.
    if(!owner_scene){last_detail.store(25,std::memory_order_relaxed);return false;}
    const auto* gate=active.load(std::memory_order_acquire);
    if(!gate){last_detail.store(3,std::memory_order_relaxed);return false;}
    const auto detail=evaluate(*gate);last_detail.store(detail,std::memory_order_relaxed);
    if(detail!=200)return false;
    if(!controlled_party_locality_override(*gate,member_user,member_actor,owner_scene)){last_detail.store(23,std::memory_order_relaxed);return false;}
    override_accepts.fetch_add(1,std::memory_order_relaxed);return true;
}
uint32_t evaluate(const NativePartyNearbyGate& gate) noexcept {
    if(gate.game_thread!=std::this_thread::get_id() || !gate.read || !gate.party_link || !gate.owner_user ||
        !gate.owner_actor || gate.owner_actor>=65534 || !gate.member_count || gate.member_count>gate.members.size())return 10;
    try {
        auto* owner_link=static_cast<std::byte*>(gate.party_link(const_cast<void*>(gate.owner_user)));
        if(!owner_link)return 11;
        uintptr_t party{};uint8_t active_flag{};uint16_t owner_uid{};
        if(!read(gate,reinterpret_cast<uintptr_t>(owner_link)+24,party) || !party ||
            !read(gate,reinterpret_cast<uintptr_t>(owner_link)+32,active_flag) || !active_flag ||
            !read(gate,reinterpret_cast<uintptr_t>(gate.owner_user)+112,owner_uid) || owner_uid!=gate.owner_actor ||
            party>std::numeric_limits<uintptr_t>::max()-80)return 12;
        std::array<uint8_t,255> seen{};
        for(std::size_t i=0;i<gate.member_count;++i) {
            const auto& member=gate.members[i];
            if(!member.user || !member.actor || member.actor>=65534 || member.actor==gate.owner_actor)return 13;
            for(std::size_t j=0;j<i;++j)if(gate.members[j].actor==member.actor)return 14;
        }
        unsigned owner_seen{},selected{};
        for(std::size_t seat=0;seat<8;++seat) {
            uint32_t actor{};if(!read(gate,party+48+seat*sizeof(actor),actor))return 15;
            if(actor==0 || actor==65535)continue;
            if(actor==gate.owner_actor){if(++owner_seen!=1)return 16;continue;}
            std::size_t index=gate.member_count;
            for(std::size_t i=0;i<gate.member_count;++i)if(actor==gate.members[i].actor){index=i;break;}
            if(index==gate.member_count)return 17;
            if(++seen[index]!=1)return 18;
            const auto& member=gate.members[index];auto* link=static_cast<std::byte*>(gate.party_link(const_cast<void*>(member.user)));
            uintptr_t member_party{};uint8_t member_active{};uint16_t uid{};
            if(!link || !read(gate,reinterpret_cast<uintptr_t>(link)+24,member_party) || member_party!=party ||
                !read(gate,reinterpret_cast<uintptr_t>(link)+32,member_active) || !member_active ||
                !read(gate,reinterpret_cast<uintptr_t>(member.user)+112,uid) || uid!=member.actor)return 19;
            ++selected;
        }
        if(owner_seen!=1)return 20;
        if(!selected)return 21;
        return 200;
    }catch(...){return 22;}
}
}
bool controlled_party_nearby_override(const NativePartyNearbyGate& gate) noexcept {
    return evaluate(gate)==200;
}
bool controlled_party_locality_override(const NativePartyNearbyGate& gate,const void* user,
    const void* member_actor,const void* owner_scene) noexcept {
    (void)member_actor; // Explicit operator policy: AI town proximity is virtual.
    return owner_scene && controlled_party_member_nearby_override(gate,user);
}
bool controlled_party_member_nearby_override(const NativePartyNearbyGate& gate,const void* user) noexcept {
    if(!user || user==gate.owner_user || evaluate(gate)!=200)return false;
    try {
        uint16_t actor{};
        for(std::size_t i=0;i<gate.member_count;++i)if(gate.members[i].user==user){actor=gate.members[i].actor;break;}
        if(!actor)return false;
        auto* link=gate.party_link(const_cast<void*>(gate.owner_user));uintptr_t party{};
        if(!link || !read(gate,reinterpret_cast<uintptr_t>(link)+24,party)||!party)return false;
        // A prepared friend not actually seated must not receive the exception.
        for(std::size_t i=0;i<8;++i){uint32_t id{};if(!read(gate,party+48+i*4,id))return false;if(id==actor)return true;}
        return false;
    }catch(...){return false;}
}
bool arm_native_party_nearby_gate(const NativePartyNearbyGate& gate) noexcept {
    if(gate.game_thread!=std::this_thread::get_id() || !installed.load(std::memory_order_acquire) ||
        !gate.read || !gate.party_link || !gate.owner_user || !gate.owner_actor || gate.owner_actor>=65534 ||
        !gate.member_count || gate.member_count>gate.members.size())return false;
    for(std::size_t i=0;i<gate.member_count;++i)if(!gate.members[i].user || !gate.members[i].actor ||
        gate.members[i].actor>=65534 || gate.members[i].actor==gate.owner_actor)return false;
    auto* copy=new(std::nothrow) NativePartyNearbyGate(gate);if(!copy)return false;
    active.store(copy,std::memory_order_release);return true;
}
void disarm_native_party_nearby_gate() noexcept {active.store(nullptr,std::memory_order_release);}
NativePartyNearbyStatus native_party_nearby_status() noexcept {
    return {installed.load(std::memory_order_acquire)?1u:0u,active.load(std::memory_order_acquire)?1u:0u,
        last_detail.load(std::memory_order_relaxed),calls.load(std::memory_order_relaxed),
        original_accepts.load(std::memory_order_relaxed),override_accepts.load(std::memory_order_relaxed)};
}
bool native_party_nearby_candidate_active() noexcept {
    const auto* gate=active.load(std::memory_order_acquire);
    return gate && gate->game_thread==std::this_thread::get_id() && controlled_party_nearby_override(*gate);
}
NativeLocalPeerStatus native_local_peer_status() noexcept {
    return {door_site.load(std::memory_order_acquire)?1u:0u,peer_detail.load(std::memory_order_relaxed),
        peer_completions.load(std::memory_order_relaxed),door_exclusions.load(std::memory_order_relaxed)};
}
void set_native_transition_callback(void(*callback)(void*)) noexcept {transition_callback.store(callback,std::memory_order_release);}
bool install_native_local_peer_gates(uintptr_t complete,uintptr_t manager_slot,uintptr_t door_call,uintptr_t command_begin,uintptr_t map_scene_call) noexcept {
    if(!complete || !manager_slot || !door_call || !command_begin || !map_scene_call){peer_detail.store(1);return false;}
    if(const auto existing=door_site.load(std::memory_order_acquire))return existing==door_call;
    constexpr std::array<unsigned char,5> before{0xe8,0x61,0x01,0x08,0x00};
    constexpr std::array<unsigned char,5> map_before{0xe8,0x45,0x1f,0x6f,0x00};
    constexpr std::array<unsigned char,15> begin_before{0x40,0x55,0x56,0x57,0x41,0x56,0x41,0x57,0x48,0x81,0xec,0x80,0,0,0};
    if(!usmod::readable(reinterpret_cast<void*>(door_call),before.size()) ||
        std::memcmp(reinterpret_cast<void*>(door_call),before.data(),before.size()) ||
        !usmod::readable(reinterpret_cast<void*>(command_begin),begin_before.size()) ||
        std::memcmp(reinterpret_cast<void*>(command_begin),begin_before.data(),begin_before.size()) ||
        !usmod::readable(reinterpret_cast<void*>(map_scene_call),map_before.size()) ||
        std::memcmp(reinterpret_cast<void*>(map_scene_call),map_before.data(),map_before.size())){peer_detail.store(2);return false;}
    try {
        int32_t rel{};std::memcpy(&rel,before.data()+1,4);
        const auto lookup=static_cast<uintptr_t>(static_cast<int64_t>(door_call+5)+rel);
        std::string reason;auto* relay=usmod::make_tail_thunk(reinterpret_cast<void*>(&door_lookup),reason);
        if(!relay){peer_detail.store(3);return false;}
        const auto distance=static_cast<int64_t>(reinterpret_cast<uintptr_t>(relay))-static_cast<int64_t>(door_call+5);
        if(distance<INT32_MIN || distance>INT32_MAX){peer_detail.store(4);return false;}
        std::vector<unsigned char> after(before.begin(),before.end());
        const auto delta=static_cast<int32_t>(distance);std::memcpy(after.data()+1,&delta,4);
        auto* original_begin=packet_trampoline(command_begin,begin_before);
        if(!original_begin){peer_detail.store(7);return false;}
        int32_t map_rel{};std::memcpy(&map_rel,map_before.data()+1,4);
        const auto map_getter=static_cast<uintptr_t>(static_cast<int64_t>(map_scene_call+5)+map_rel);
        auto* map_stub=usmod::make_post_call_stub(reinterpret_cast<void*>(map_scene_call),reinterpret_cast<void*>(map_getter),observe_transition,reason);
        if(!map_stub){peer_detail.store(8);return false;}
        const auto map_distance=static_cast<int64_t>(reinterpret_cast<uintptr_t>(map_stub))-static_cast<int64_t>(map_scene_call+5);
        if(map_distance<INT32_MIN || map_distance>INT32_MAX){peer_detail.store(9);return false;}
        std::vector<unsigned char> map_after(map_before.begin(),map_before.end());
        const auto map_delta=static_cast<int32_t>(map_distance);std::memcpy(map_after.data()+1,&map_delta,4);
        std::vector<unsigned char> begin_after(begin_before.size(),0x90);absolute_jump(begin_after.data(),reinterpret_cast<uintptr_t>(&packet_begin));
        original_door_lookup.store(reinterpret_cast<PartyUserLookup>(lookup),std::memory_order_release);
        original_packet_begin.store(reinterpret_cast<PacketBegin>(original_begin),std::memory_order_release);
        if(!usmod::apply({{reinterpret_cast<void*>(door_call),{before.begin(),before.end()},after},
            {reinterpret_cast<void*>(command_begin),{begin_before.begin(),begin_before.end()},begin_after},
            {reinterpret_cast<void*>(map_scene_call),{map_before.begin(),map_before.end()},map_after}},reason)){
            peer_detail.store(5);return false;
        }
        complete_local_peer.store(reinterpret_cast<CompleteLocalPeer>(complete),std::memory_order_release);
        peer_manager_slot.store(manager_slot,std::memory_order_release);
        door_site.store(door_call,std::memory_order_release);peer_detail.store(0);return true;
    }catch(...){peer_detail.store(6);return false;}
}
void maintain_native_local_peers() noexcept {
    const auto* gate=active.load(std::memory_order_acquire);
    const auto complete=complete_local_peer.load(std::memory_order_acquire);
    const auto slot=peer_manager_slot.load(std::memory_order_acquire);
    if(!gate || !complete || !slot || gate->game_thread!=std::this_thread::get_id())return;
    try {
        uintptr_t manager{},party{};
        if(!read(*gate,slot,manager) || !manager || manager>std::numeric_limits<uintptr_t>::max()-1090741)return;
        // Steady-state is just a pointer and eight bytes. Full owned-party
        // verification runs only when a peer actually waits for a reply.
        std::array<uint8_t,8> waiting{};
        if(!read(*gate,manager+1090733,waiting))return;
        bool any{};for(const auto value:waiting)any|=value==1;
        if(!any || evaluate(*gate)!=200)return;
        auto* link=gate->party_link(const_cast<void*>(gate->owner_user));
        if(!link || !read(*gate,reinterpret_cast<uintptr_t>(link)+24,party) || !party)return;
        for(uint32_t seat=0;seat<8;++seat) {
            uint32_t uid{};if(!read(*gate,party+48+seat*4,uid))return;
            if(!uid || uid==65535 || uid==gate->owner_actor)continue;
            const void* user{};
            for(size_t n=0;n<gate->member_count;++n)if(gate->members[n].actor==uid){user=gate->members[n].user;break;}
            if(!user || !controlled_party_member_nearby_override(*gate,user))continue;
            // Native sync-start sets this per-seat pending flag. The reviewed
            // local completion handler clears it (the normal remote path waits
            // up to12seconds). No peer socket is fabricated for a local Actor.
            uint8_t pending{};
            if(!read(*gate,manager+1090733+seat,pending) || pending>1){peer_detail.store(10);return;}
            if(!pending)continue;
            complete(reinterpret_cast<void*>(manager),static_cast<int32_t>(seat),const_cast<void*>(user));
            uintptr_t current{};uint8_t after{};
            if(!read(*gate,slot,current) || current!=manager ||
                !read(*gate,manager+1090733+seat,after) || after){peer_detail.store(11);return;}
            peer_completions.fetch_add(1,std::memory_order_relaxed);peer_detail.store(0);
        }
    }catch(...){peer_detail.store(12);}
}
bool build_native_party_selection_patch(uintptr_t entry,uintptr_t relay,std::array<std::byte,33>& out) noexcept {
    if(!entry || !relay || entry>std::numeric_limits<uintptr_t>::max()-33)return false;
    const auto delta=static_cast<int64_t>(relay)-static_cast<int64_t>(entry+14);
    if(delta<INT32_MIN||delta>INT32_MAX)return false;
    out.fill(std::byte{0x90});
    const unsigned char args[]{0x48,0x8b,0xcb,0x49,0x8b,0xd6,0x4c,0x8b,0xc0,0xe8};
    std::memcpy(out.data(),args,sizeof(args));
    const int32_t relative=static_cast<int32_t>(delta);std::memcpy(out.data()+10,&relative,4);
    const unsigned char branch[]{0x84,0xc0,0x0f,0x85};std::memcpy(out.data()+14,branch,sizeof(branch));
    const int32_t pass=0x10a-22;std::memcpy(out.data()+18,&pass,4);
    return true;
}
bool install_native_party_nearby_gate(uintptr_t entry) noexcept {
    if(!entry || entry>std::numeric_limits<uintptr_t>::max()-prologue.size())return false;
    const auto existing=installed.load(std::memory_order_acquire);if(existing)return existing==entry;
    if(!usmod::readable(reinterpret_cast<void*>(entry),prologue.size()) ||
       std::memcmp(reinterpret_cast<void*>(entry),prologue.data(),prologue.size())){last_detail.store(50);return false;}
    try {
        int32_t old_relative{};std::memcpy(&old_relative,prologue.data()+21,4);
        const auto compare_address=static_cast<uintptr_t>(static_cast<int64_t>(entry+25)+old_relative);
        original_compare.store(reinterpret_cast<SceneCompare>(compare_address),std::memory_order_release);
        std::string reason;
        auto* relay=usmod::make_tail_thunk(reinterpret_cast<void*>(&hooked),reason);
        if(!relay){last_detail.store(51);return false;}
        // The original native block itself contains a call, with the parent's
        // shadow space/alignment already established. No return-address edits.
        std::array<std::byte,33> built{};
        if(!build_native_party_selection_patch(entry,reinterpret_cast<uintptr_t>(relay),built)){last_detail.store(52);return false;}
        const auto* bytes=reinterpret_cast<const unsigned char*>(built.data());
        std::vector<unsigned char> patch(bytes,bytes+built.size());
        const std::vector<usmod::Write> writes{{reinterpret_cast<void*>(entry),
            {prologue.begin(),prologue.end()},patch}};
        if(!usmod::apply(writes,reason)){last_detail.store(53);return false;}
        installed.store(entry,std::memory_order_release);last_detail.store(0);
        return true;
    }catch(...){last_detail.store(54);return false;}
}
}

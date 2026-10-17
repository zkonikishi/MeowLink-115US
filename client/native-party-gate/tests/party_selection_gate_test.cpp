#include "native_party_nearby_gate.hpp"
#include <array>
#include <cstring>
#include <cstdio>
using namespace us115::ai;
struct User {std::array<std::byte,160> bytes{};std::array<std::byte,48> link{};};
struct Fixture {
 User owner,ai,spare;std::array<std::byte,80> party{},other{};NativePartyNearbyGate gate;
 template<class T> static void put(std::byte* p,T v){std::memcpy(p,&v,sizeof(v));}
 static bool read(void* ctx,const void* source,void* out,size_t n){
  auto& f=*static_cast<Fixture*>(ctx);const auto at=reinterpret_cast<uintptr_t>(source);
  struct Region{const void* p;size_t n;};
  for(auto r:{Region{&f.owner,sizeof(f.owner)},Region{&f.ai,sizeof(f.ai)},Region{&f.spare,sizeof(f.spare)},Region{f.party.data(),f.party.size()},Region{f.other.data(),f.other.size()}}){
   const auto lo=reinterpret_cast<uintptr_t>(r.p);if(at>=lo&&r.n>=n&&at-lo<=r.n-n){std::memcpy(out,source,n);return true;}}
  return false;
 }
 static void* link(void* p){return static_cast<User*>(p)->link.data();}
 void seat(size_t n,uint32_t id){put(party.data()+48+n*4,id);}
 Fixture(){
  for(auto* u:{&owner,&ai,&spare}){put(u->link.data()+24,reinterpret_cast<uintptr_t>(party.data()));put<uint8_t>(u->link.data()+32,1);}
  put<uint16_t>(owner.bytes.data()+112,2);put<uint16_t>(ai.bytes.data()+112,65533);put<uint16_t>(spare.bytes.data()+112,65532);
  for(size_t i=0;i<8;++i)seat(i,65535);seat(0,2);seat(1,65533);
  gate={std::this_thread::get_id(),read,this,link,&owner,2};gate.member_count=1;gate.members[0]={&ai,65533};
 }
};
int main(){
 {Fixture f;int scene{},native_ref{};
  if(!controlled_party_locality_override(f.gate,&f.ai,nullptr,&scene) ||
     !controlled_party_locality_override(f.gate,&f.ai,&native_ref,&scene))return 9;
  if(controlled_party_locality_override(f.gate,&f.ai,&native_ref,nullptr) ||
     controlled_party_locality_override(f.gate,&f.spare,&native_ref,&scene) ||
     controlled_party_locality_override(f.gate,&f.owner,&native_ref,&scene))return 10;
  f.seat(2,9);
  if(controlled_party_locality_override(f.gate,&f.ai,&native_ref,&scene))return 11;
 }
 {Fixture f;if(!controlled_party_member_nearby_override(f.gate,&f.ai))return 1;
  if(controlled_party_member_nearby_override(f.gate,&f.owner)||controlled_party_member_nearby_override(f.gate,&f.spare))return 2;}
 {Fixture f;f.seat(2,9);if(controlled_party_member_nearby_override(f.gate,&f.ai))return 3;}
 {Fixture f;f.seat(2,65533);if(controlled_party_member_nearby_override(f.gate,&f.ai))return 4;}
 {Fixture f;Fixture::put<uintptr_t>(f.ai.link.data()+24,reinterpret_cast<uintptr_t>(f.other.data()));if(controlled_party_member_nearby_override(f.gate,&f.ai))return 5;}
 {Fixture f;f.gate.game_thread={};if(controlled_party_member_nearby_override(f.gate,&f.ai))return 6;}
 {Fixture f;f.gate.member_count=2;f.gate.members[1]={&f.spare,65532};if(controlled_party_member_nearby_override(f.gate,&f.spare)||!controlled_party_member_nearby_override(f.gate,&f.ai))return 7;}
 {Fixture f;Fixture::put<uint16_t>(f.ai.bytes.data()+112,7);if(controlled_party_member_nearby_override(f.gate,&f.ai))return 8;}
 std::puts("PASS: exact seated AI locality bypass with absent OR present Actor; owner, unknown/human, duplicate, stale party/UID, wrong thread and unselected friend rejected");
}

#define NOMINMAX
#include <windows.h>
#include "native_party_nearby_gate.hpp"
#include <cstdio>
#include <cstring>
static bool gate_accept{},arguments{};
static bool probe(void* user,void* actor,void* owner){arguments=user==reinterpret_cast<void*>(0x1111)&&actor==reinterpret_cast<void*>(0x2222)&&owner==reinterpret_cast<void*>(0x3333);return gate_accept;}
int main(){
 auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));unsigned char* memory{};
 for(uintptr_t at=base+0x100000;at<base+0x10000000&&!memory;at+=0x10000)memory=static_cast<unsigned char*>(VirtualAlloc(reinterpret_cast<void*>(at),4096,MEM_RESERVE|MEM_COMMIT,PAGE_READWRITE));
 if(!memory)return 1;auto entry=memory+128;
 const unsigned char prefix[]{0x53,0x41,0x56,0x48,0x83,0xec,0x28,0x48,0x8b,0xd9,0x4c,0x8b,0xf2,0x49,0x8b,0xc0,0xe9};
 std::memcpy(memory,prefix,sizeof(prefix));const int32_t next=128-static_cast<int32_t>(sizeof(prefix))-4;std::memcpy(memory+sizeof(prefix),&next,4);
 std::array<std::byte,33> patch{};if(!us115::ai::build_native_party_selection_patch(reinterpret_cast<uintptr_t>(entry),reinterpret_cast<uintptr_t>(&probe),patch))return 2;
 std::memcpy(entry,patch.data(),patch.size());
 const unsigned char fail[]{0x33,0xc0,0x48,0x83,0xc4,0x28,0x41,0x5e,0x5b,0xc3};
 const unsigned char pass[]{0xb8,1,0,0,0,0x48,0x83,0xc4,0x28,0x41,0x5e,0x5b,0xc3};
 std::memcpy(entry+33,fail,sizeof(fail));std::memcpy(entry+0x10a,pass,sizeof(pass));DWORD old{};if(!VirtualProtect(memory,4096,PAGE_EXECUTE_READ,&old))return 3;
 auto fn=reinterpret_cast<int(*)(void*,void*,void*)>(memory);
 for(bool value:{false,true}){gate_accept=value;arguments=false;auto got=fn(reinterpret_cast<void*>(0x1111),reinterpret_cast<void*>(0x2222),reinterpret_cast<void*>(0x3333));if(got!=static_cast<int>(value)||!arguments)return 4;}
 VirtualFree(memory,0,MEM_RELEASE);std::puts("PASS: actual 33-byte patch transfers User/Actor/owner arguments and both native continuation branches");
}

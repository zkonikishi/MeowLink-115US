#define WIN32_LEAN_AND_MEAN
#define _CRT_SECURE_NO_WARNINGS
#include <windows.h>
#include <tlhelp32.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>

/* One draw-local call at 14553f6b9. Its return is ONLY tested for null;
 * it is never stored as a party member or dereferenced. Preserve real actors
 * and the native disconnected-member queue. Suppress only truly empty slots.
 * Server count, slot indexes, party capacity and gameplay are untouched. */
#define COUNT(a) (sizeof(a)/sizeof((a)[0]))
#define CALL_RVA 0x553f6b9
#define WINDOW_MANAGER_RVA 0xe684c98
static HMODULE self;
static unsigned char *image;
static LONG started, hidden;
static BOOL enabled;
static wchar_t directory[MAX_PATH];
typedef void *(__fastcall *MemberAt)(void *, unsigned);
typedef void *(__fastcall *CurrentParty)(void);
typedef uintptr_t *(__fastcall *ReserveList)(void *);
typedef void (__fastcall *SetFlag)(void *, BOOL);

static BOOL readable(const void *p, SIZE_T size) {
    MEMORY_BASIC_INFORMATION m;
    uintptr_t a=(uintptr_t)p;
    return p && VirtualQuery(p,&m,sizeof(m))==sizeof(m) && m.State==MEM_COMMIT &&
        !(m.Protect&(PAGE_NOACCESS|PAGE_GUARD)) && a+size>=a &&
        a+size<=(uintptr_t)m.BaseAddress+m.RegionSize;
}
static void status(const char *reason) {
    wchar_t path[MAX_PATH]; FILE *f;
    _snwprintf_s(path,COUNT(path),_TRUNCATE,L"%slegion-party-hud-%lu.status.json",directory,GetCurrentProcessId());
    f=_wfopen(path,L"wb");
    if(f){fprintf(f,"{\"version\":\"0.1.0\",\"pid\":%lu,\"enabled\":%s,\"hiddenEmptySlots\":%ld,\"reason\":\"%s\"}\n",GetCurrentProcessId(),enabled?"true":"false",hidden,reason);fclose(f);}
}
static void *__fastcall drawMember(void *manager,unsigned slot,uintptr_t cursor,void *widget) {
    void *actor=((MemberAt)(image+0x5f13fc0))(manager,slot);
    void *party; uintptr_t *reserves,*vt; short mode;
    if(actor || slot>=4)return actor;
    party=((CurrentParty)(image+0x5f0c150))();
    if(!readable(party,0x84) || !readable(widget,sizeof(void*)))return actor;
    mode=*(short*)((unsigned char*)party+8+122);
    if(mode!=11 && mode!=13 && mode!=16 && mode!=34 && mode!=38)return actor;
    reserves=((ReserveList)(image+0x5f10090))(party);
    if(!readable(reserves,24) || reserves[0]>reserves[1] || reserves[1]>reserves[2] ||
       reserves[1]-reserves[0]>8*56 || cursor!=reserves[1])return actor;
    vt=*(uintptr_t**)widget;
    if(!readable(vt,24))return actor;
    ((SetFlag)vt[2])(widget,FALSE);
    if(InterlockedIncrement(&hidden)==1)status("");
    /* Non-null is a local draw-skip predicate, never an actor publication. */
    return (void*)(uintptr_t)1;
}
static BOOL preflight(void) {
    static const unsigned char site[]={0xe8,0x02,0x49,0x9d,0x00,0x48,0x85,0xc0,0x0f,0x85,0x35,0x04,0x00,0x00};
    static const unsigned char frame[]={0x49,0x8b,0x4d,0x00,0x48,0x8b,0x01,0xb2,0x01,0xff,0x50,0x10};
    static const unsigned char member[]={0x83,0xfa,0x07,0x77,0x23,0x48,0x63,0xc2,0x48,0x8d,0x14,0x40,0x48,0x8b,0x84,0xd1};
    static const unsigned char party[]={0x40,0x53,0x48,0x83,0xec,0x20,0x48,0x8b,0x1d,0xcb,0x8a,0x77,0x08,0x48,0x85,0xdb};
    static const unsigned char reserve[]={0x48,0x8d,0x81,0x50,0x02,0x00,0x00,0xc3};
    return readable(image+CALL_RVA,sizeof(site)) && !memcmp(image+CALL_RVA,site,sizeof(site)) &&
        readable(image+0x553f6c7,sizeof(frame)) && !memcmp(image+0x553f6c7,frame,sizeof(frame)) &&
        readable(image+0x5f13fc0,sizeof(member)) && !memcmp(image+0x5f13fc0,member,sizeof(member)) &&
        readable(image+0x5f0c150,sizeof(party)) && !memcmp(image+0x5f0c150,party,sizeof(party)) &&
        readable(image+0x5f10090,sizeof(reserve)) && !memcmp(image+0x5f10090,reserve,sizeof(reserve));
}
static unsigned char *nearPage(void) {
    uintptr_t at=(uintptr_t)image&~(uintptr_t)0xffff,distance;
    for(distance=0x10000;distance<0x70000000;distance+=0x10000){
        MEMORY_BASIC_INFORMATION m; unsigned char *p=(unsigned char*)(at+distance);
        if(VirtualQuery(p,&m,sizeof(m))==sizeof(m) && m.State==MEM_FREE){
            p=VirtualAlloc(p,4096,MEM_COMMIT|MEM_RESERVE,PAGE_READWRITE);
            if(p)return p;
        }
    }
    return NULL;
}
static void resume(HANDLE *threads,unsigned count) {
    while(count){HANDLE h=threads[--count];ResumeThread(h);CloseHandle(h);}
}
static BOOL freeze(HANDLE *threads,unsigned *count) {
    THREADENTRY32 e; BOOL ok; HANDLE snapshot=CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0);
    *count=0;
    if(snapshot==INVALID_HANDLE_VALUE)return FALSE;
    e.dwSize=sizeof(e);ok=Thread32First(snapshot,&e);
    while(ok){
        if(e.th32OwnerProcessID==GetCurrentProcessId() && e.th32ThreadID!=GetCurrentThreadId()){
            HANDLE h;
            if(*count==256)goto fail;
            h=OpenThread(THREAD_SUSPEND_RESUME,FALSE,e.th32ThreadID);
            if(!h)goto fail;
            if(SuspendThread(h)==(DWORD)-1){CloseHandle(h);goto fail;}
            threads[(*count)++]=h;
        }
        ok=Thread32Next(snapshot,&e);
    }
    CloseHandle(snapshot);return TRUE;
fail:
    CloseHandle(snapshot);resume(threads,*count);*count=0;return FALSE;
}
__declspec(dllexport) const char *WINAPI ModName(void){return "Legion real party HUD";}
__declspec(dllexport) DWORD WINAPI ModStart(void) {
    HANDLE threads[256]; unsigned count=0; DWORD old; unsigned char *page; wchar_t *slash;
    uintptr_t target=(uintptr_t)drawMember; intptr_t delta; int32_t rel; unsigned char call[5]={0xe8};
    /* R8 = native reserve cursor (RBX); R9 = this slot widget ([R13]).
     * Tail jump preserves the caller's return address, shadow space and all
     * nonvolatile registers under the ordinary Windows x64 calling ABI. */
    static const unsigned char relay[]={0x49,0x89,0xd8,0x4d,0x8b,0x4d,0x00,0x48,0xb8};
    if(InterlockedCompareExchange(&started,1,0))return enabled?0:1;
    if(!GetModuleFileNameW(self,directory,COUNT(directory)))return 1;
    slash=wcsrchr(directory,L'\\');if(!slash)return 1;slash[1]=0;
    image=(unsigned char*)GetModuleHandleW(NULL);
    if(!preflight()){status("instruction-mismatch");return 1;}
    page=nearPage();if(!page){status("relay-unavailable");return 1;}
    memcpy(page,relay,sizeof(relay));memcpy(page+sizeof(relay),&target,8);
    page[17]=0xff;page[18]=0xe0;
    delta=(intptr_t)page-(intptr_t)(image+CALL_RVA+5);
    if(delta<INT32_MIN || delta>INT32_MAX || !VirtualProtect(page,4096,PAGE_EXECUTE_READ,&old)){
        VirtualFree(page,0,MEM_RELEASE);status("relay-protection-failed");return 1;
    }
    FlushInstructionCache(GetCurrentProcess(),page,4096);
    rel=(int32_t)delta;memcpy(call+1,&rel,4);
    if(!freeze(threads,&count)){VirtualFree(page,0,MEM_RELEASE);status("thread-freeze-failed");return 1;}
    if(readable(image+WINDOW_MANAGER_RVA,sizeof(void*)) && !*(void**)(image+WINDOW_MANAGER_RVA) && preflight() &&
       VirtualProtect(image+CALL_RVA,5,PAGE_EXECUTE_READWRITE,&old)){
        DWORD ignored;
        memcpy(image+CALL_RVA,call,5);FlushInstructionCache(GetCurrentProcess(),image+CALL_RVA,5);
        VirtualProtect(image+CALL_RVA,5,old,&ignored);enabled=TRUE;
    }
    resume(threads,count);
    if(!enabled)VirtualFree(page,0,MEM_RELEASE);
    status(enabled?"":"late-load-or-preflight-failed");return enabled?0:1;
}
BOOL WINAPI DllMain(HINSTANCE h,DWORD reason,LPVOID reserved) {
    (void)reserved;
    if(reason==DLL_PROCESS_ATTACH){self=h;DisableThreadLibraryCalls(h);}
    return TRUE;
}

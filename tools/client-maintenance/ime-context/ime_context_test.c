#define IME_CONTEXT_TEST
#include "ime_context.c"
#include <assert.h>
#include <stdio.h>

static HIMC slot;static int creates,destroys,associations,gets,releases;
static DWORD windowOwner=7,pid=9,thread=7;
static BOOL valid=TRUE,failCreate=FALSE,failAssociate=FALSE;
static BOOL failDestroy=FALSE;static DWORD reentrantResult;
static BOOL WINAPI isWindow_(HWND h){return valid&&h==(HWND)1;}
static DWORD WINAPI windowThread_(HWND h,LPDWORD p){(void)h;*p=pid;return windowOwner;}
static DWORD WINAPI thread_(void){return thread;}
static DWORD WINAPI process_(void){return 9;}
static HIMC WINAPI get_(HWND h){(void)h;gets++;return slot;}
static BOOL WINAPI release_(HWND h,HIMC c){(void)h;(void)c;releases++;return TRUE;}
static HIMC WINAPI create_(void){creates++;return failCreate?NULL:(HIMC)2;}
static HIMC WINAPI associate_(HWND h,HIMC c){HIMC before=slot;(void)h;associations++;if(!failAssociate)slot=c;return before;}
static BOOL WINAPI destroy_(HIMC c){assert(c==(HIMC)2);destroys++;reentrantResult=MeowImeRecover();return !failDestroy;}

int main(void){
 HWND h=(HWND)1;
 ops=(ImeOps){isWindow_,windowThread_,thread_,process_,get_,release_,create_,associate_,destroy_};
 assert(ModStart()==IME_BOOTSTRAP_REQUIRED&&MeowImeApiVersion()==1);
 assert(MeowImeAttach(NULL)==IME_INVALID_WINDOW&&creates==0);
 pid=10;assert(MeowImeAttach(h)==IME_INVALID_WINDOW);pid=9;
 thread=8;assert(MeowImeAttach(h)==IME_WRONG_THREAD&&creates==0);thread=7;
 slot=(HIMC)3;assert(MeowImeAttach(h)==IME_ALREADY_AVAILABLE&&creates==0&&associations==0&&slot==(HIMC)3);slot=NULL;
 failCreate=TRUE;assert(MeowImeAttach(h)==IME_CREATE_FAILED&&!ownedContext);failCreate=FALSE;
 failAssociate=TRUE;assert(MeowImeAttach(h)==IME_ASSOCIATE_FAILED&&!ownedContext&&destroys==1);failAssociate=FALSE;
 assert(MeowImeAttach(h)==IME_OK&&slot==(HIMC)2);
 assert(MeowImeAttach(h)==IME_BUSY);
 thread=8;assert(MeowImeDetach(h)==IME_WRONG_THREAD&&slot==(HIMC)2);thread=7;
 slot=(HIMC)4;assert(MeowImeDetach(h)==IME_OWNERSHIP_CHANGED&&slot==(HIMC)4&&destroys==1);slot=(HIMC)2;
 assert(MeowImeDetach(h)==IME_OK&&slot==NULL&&ownedContext==NULL&&destroys==2);
 assert(MeowImeDetach(h)==IME_OK);
 AcquireSRWLockExclusive(&stateLock);assert(MeowImeAttach(h)==IME_BUSY);ReleaseSRWLockExclusive(&stateLock);
 assert(MeowImeAttach(h)==IME_OK);
 failAssociate=TRUE;assert(MeowImeDetach(h)==IME_CLEANUP_PENDING&&ownedContext==(HIMC)2&&slot==(HIMC)2);failAssociate=FALSE;
 assert(MeowImeDetach(h)==IME_OK&&!ownedContext);
 assert(reentrantResult==IME_BUSY);
 assert(MeowImeAttach(h)==IME_OK);
 failDestroy=TRUE;assert(MeowImeDetach(h)==IME_CLEANUP_PENDING&&slot==NULL&&detachedContext);
 assert(MeowImeAttach(h)==IME_BUSY);
 thread=8;assert(MeowImeRecover()==IME_WRONG_THREAD);thread=7;
 valid=FALSE;assert(MeowImeDetach(h)==IME_INVALID_WINDOW);
 assert(MeowImeRecover()==IME_CLEANUP_PENDING);
 failDestroy=FALSE;assert(MeowImeRecover()==IME_OK&&!ownedContext&&!detachedContext);
 valid=TRUE;assert(MeowImeRecover()==IME_OK);
 printf("PASS fixture: missing bootstrap, invalid/foreign HWND, wrong thread, existing context preserved, create/associate failure, attach/detach, duplicate/reentrant busy, changed ownership, cleanup pending/retry; no window/game created.\n");
 return 0;
}

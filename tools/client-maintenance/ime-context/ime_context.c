/* Author: zkonikishi. Explicit UI-thread IME context repair candidate.
 * No EAT/IAT/inline patches, window procedure replacement or worker thread.
 * Host retains this DLL and calls Detach before destroying the bound HWND.
 * An existing context is left entirely under the game's ownership.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <imm.h>

enum { IME_OK=0, IME_ALREADY_AVAILABLE=1, IME_BOOTSTRAP_REQUIRED=2,
       IME_INVALID_WINDOW=10, IME_WRONG_THREAD=11, IME_BUSY=12,
       IME_CREATE_FAILED=13, IME_ASSOCIATE_FAILED=14,
       IME_OWNERSHIP_CHANGED=15, IME_CLEANUP_PENDING=16 };

typedef struct {
 BOOL (WINAPI *isWindow)(HWND);
 DWORD (WINAPI *windowThread)(HWND, LPDWORD);
 DWORD (WINAPI *threadId)(void);
 DWORD (WINAPI *processId)(void);
 HIMC (WINAPI *get)(HWND);
 BOOL (WINAPI *release)(HWND,HIMC);
 HIMC (WINAPI *create)(void);
 HIMC (WINAPI *associate)(HWND,HIMC);
 BOOL (WINAPI *destroy)(HIMC);
} ImeOps;

#ifndef IME_CONTEXT_TEST
static BOOL WINAPI nativeIsWindow(HWND h){return IsWindow(h);}
static DWORD WINAPI nativeWindowThread(HWND h,LPDWORD p){return GetWindowThreadProcessId(h,p);}
static DWORD WINAPI nativeThread(void){return GetCurrentThreadId();}
static DWORD WINAPI nativeProcess(void){return GetCurrentProcessId();}
static const ImeOps ops={nativeIsWindow,nativeWindowThread,nativeThread,
 nativeProcess,ImmGetContext,ImmReleaseContext,ImmCreateContext,
 ImmAssociateContext,ImmDestroyContext};
#else
static ImeOps ops;
#endif

static SRWLOCK stateLock=SRWLOCK_INIT;
static HWND boundWindow;
static HIMC ownedContext;
static DWORD boundThread;
static BOOL detachedContext;

static DWORD destroyDetached(void) {
 if(!ownedContext)return IME_OK;
 if(!detachedContext)return IME_OWNERSHIP_CHANGED;
 if(!ops.destroy(ownedContext))return IME_CLEANUP_PENDING;
 ownedContext=NULL;boundWindow=NULL;boundThread=0;detachedContext=FALSE;
 return IME_OK;
}

/* Recovery is valid even after HWND destruction, but only for a context proven
 * disassociated earlier and on the original creating UI thread. */
__declspec(dllexport) DWORD WINAPI MeowImeRecover(void) {
 DWORD result;
 if(!TryAcquireSRWLockExclusive(&stateLock))return IME_BUSY;
 if(!ownedContext)result=IME_OK;
 else if(boundThread!=ops.threadId())result=IME_WRONG_THREAD;
 else result=destroyDetached();
 ReleaseSRWLockExclusive(&stateLock);return result;
}

static DWORD checkWindow(HWND hwnd) {
 DWORD pid=0,thread;
 if(!hwnd||!ops.isWindow(hwnd))return IME_INVALID_WINDOW;
 thread=ops.windowThread(hwnd,&pid);
 if(pid!=ops.processId())return IME_INVALID_WINDOW;
 if(!thread||thread!=ops.threadId())return IME_WRONG_THREAD;
 return IME_OK;
}

/* Must be invoked directly by trusted host bootstrap on the target UI thread.
 * HWND is an in-process host argument, never an untrusted network parameter.
 * Capacity is one window; fail explicitly rather than silently stealing another.
 */
__declspec(dllexport) DWORD WINAPI MeowImeAttach(HWND hwnd) {
 DWORD result; HIMC existing,created,displaced,observed;
 result=checkWindow(hwnd);if(result)return result;
 if(!TryAcquireSRWLockExclusive(&stateLock))return IME_BUSY;
 if(ownedContext){result=IME_BUSY;goto done;}
 existing=ops.get(hwnd);
 if(existing){ops.release(hwnd,existing);result=IME_ALREADY_AVAILABLE;goto done;}
 created=ops.create();if(!created){result=IME_CREATE_FAILED;goto done;}
 /* Publish ownership before association, so reentrant calls see BUSY. */
 boundWindow=hwnd;boundThread=ops.threadId();ownedContext=created;detachedContext=FALSE;
 displaced=ops.associate(hwnd,created);
 observed=ops.get(hwnd);
 if(observed)ops.release(hwnd,observed);
 if(observed!=created){
  /* If we displaced another context, restore it only when our context still
   * owns the slot. On unexpected state preserve resources for host recovery. */
  if(observed||displaced){result=IME_CLEANUP_PENDING;goto done;}
  detachedContext=TRUE;
  result=destroyDetached();if(result!=IME_OK)goto done;
  result=IME_ASSOCIATE_FAILED;goto done;
 }
 if(displaced){
  /* The no-context precondition changed reentrantly. Restore the previous
   * context; never destroy a context owned by the game or another plugin. */
  ops.associate(hwnd,displaced);
  observed=ops.get(hwnd);if(observed)ops.release(hwnd,observed);
  if(observed!=displaced){result=IME_CLEANUP_PENDING;goto done;}
  detachedContext=TRUE;
  result=destroyDetached();if(result!=IME_OK)goto done;
  result=IME_OWNERSHIP_CHANGED;goto done;
 }
 result=IME_OK;
done: ReleaseSRWLockExclusive(&stateLock);return result;
}

/* Host must call before HWND destruction, and before releasing the DLL.
 * The unchanged UI thread is required. Unexpected ownership is not overwritten.
 * CLEANUP_PENDING means retain DLL/state; it does not mean successful unload.
 */
__declspec(dllexport) DWORD WINAPI MeowImeDetach(HWND hwnd) {
 DWORD result=checkWindow(hwnd);HIMC current,removed;
 if(result)return result;
 if(!TryAcquireSRWLockExclusive(&stateLock))return IME_BUSY;
 if(!ownedContext){result=IME_OK;goto done;}
 if(boundWindow!=hwnd||boundThread!=ops.threadId()){result=IME_OWNERSHIP_CHANGED;goto done;}
 if(detachedContext){result=destroyDetached();goto done;}
 current=ops.get(hwnd);if(current)ops.release(hwnd,current);
 if(current!=ownedContext){result=IME_OWNERSHIP_CHANGED;goto done;}
 removed=ops.associate(hwnd,NULL);
 current=ops.get(hwnd);if(current)ops.release(hwnd,current);
 if(removed!=ownedContext||current){result=IME_CLEANUP_PENDING;goto done;}
 detachedContext=TRUE;result=destroyDetached();
done: ReleaseSRWLockExclusive(&stateLock);return result;
}

/* The ordinary loader ABI supplies no trusted HWND/UI-thread callback.
 * Loading this module alone explicitly reports that missing capability. */
__declspec(dllexport) DWORD WINAPI ModStart(void){return IME_BOOTSTRAP_REQUIRED;}
__declspec(dllexport) const char *WINAPI ModName(void){return "meowscope-ime-context";}
__declspec(dllexport) DWORD WINAPI MeowImeApiVersion(void){return 1;}
BOOL WINAPI DllMain(HINSTANCE self,DWORD reason,LPVOID reserved){
 (void)reserved;if(reason==DLL_PROCESS_ATTACH)DisableThreadLibraryCalls(self);return TRUE;
}

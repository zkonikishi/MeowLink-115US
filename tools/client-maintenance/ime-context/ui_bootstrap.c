/* Author: zkonikishi. Process-local, single-target UI thread bootstrap.
 * WH_CALLWNDPROC observes messages without replacing any window procedure.
 * DLL is pinned before publication: no hot unload / callback into freed code.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

DWORD WINAPI MeowImeAttach(HWND);
DWORD WINAPI MeowImeDetach(HWND);
DWORD WINAPI MeowImeRecover(void);

enum { BOOT_TIMEOUT=20, BOOT_HOOK_FAILED=21, BOOT_DESTROYED=22,
       BOOT_STALE=23 };
static SRWLOCK bootLock=SRWLOCK_INIT;
static HWND targetWindow;
static DWORD targetThread;
static HHOOK threadHook;
static UINT commandMessage;
static ULONG_PTR generation;
static DWORD lastResult=2;
static BOOL destroying;
static DWORD installerThread;
static HMODULE pinnedModule;

static LRESULT CALLBACK uiHook(int code,WPARAM w,LPARAM l) {
 CWPSTRUCT *message;HWND hwnd=NULL;DWORD action=0;ULONG_PTR ticket=0;
 if(code>=0&&l){
  message=(CWPSTRUCT*)l;
  if(TryAcquireSRWLockExclusive(&bootLock)){
   if(message->hwnd==targetWindow&&GetCurrentThreadId()==targetThread){
    hwnd=targetWindow;ticket=generation;
    if(message->message==commandMessage&&message->wParam==ticket&&!destroying&&
       (message->lParam==1||message->lParam==2))
     action=(DWORD)message->lParam;
    if(message->message==WM_DESTROY){destroying=TRUE;action=2;}
    if(message->message==WM_NCDESTROY){destroying=TRUE;action=3;}
   }
   ReleaseSRWLockExclusive(&bootLock);
  }
  if(action){
   DWORD result;
   /* Hooks belong to their installing thread. Transfer ownership to the UI
    * thread before the requesting startup worker is allowed to exit. */
   if(action==1){
    AcquireSRWLockExclusive(&bootLock);
    if(installerThread!=GetCurrentThreadId()){
     HHOOK replacement=SetWindowsHookExW(WH_CALLWNDPROC,uiHook,pinnedModule,targetThread);
     if(!replacement||!UnhookWindowsHookEx(threadHook)){
      if(replacement)UnhookWindowsHookEx(replacement);
      lastResult=BOOT_HOOK_FAILED;ReleaseSRWLockExclusive(&bootLock);
      return CallNextHookEx(NULL,code,w,l);
     }
     threadHook=replacement;installerThread=GetCurrentThreadId();
    }
    ReleaseSRWLockExclusive(&bootLock);
   }
   result=action==1?MeowImeAttach(hwnd):
                action==3?MeowImeRecover():MeowImeDetach(hwnd);
   AcquireSRWLockExclusive(&bootLock);
   if(ticket==generation&&hwnd==targetWindow){
    lastResult=result;
    if(message->message==WM_NCDESTROY){targetWindow=NULL;lastResult=result?result:BOOT_DESTROYED;}
   }
   ReleaseSRWLockExclusive(&bootLock);
  }
 }
 return CallNextHookEx(NULL,code,w,l);
}

/* Host chooses exact HWND; module never enumerates or guesses game windows.
 * A timeout retains target/hook rather than freeing potentially in-flight code.
 */
__declspec(dllexport) DWORD WINAPI MeowImeBootstrap(HWND hwnd) {
 DWORD pid=0,tid;HMODULE module;DWORD_PTR ignored;HHOOK hook;UINT msg;ULONG_PTR ticket;
 if(!IsWindow(hwnd))return 10;
 tid=GetWindowThreadProcessId(hwnd,&pid);
 if(!tid||pid!=GetCurrentProcessId())return 10;
 if(!TryAcquireSRWLockExclusive(&bootLock))return 12;
 if(threadHook||targetWindow){ReleaseSRWLockExclusive(&bootLock);return 12;}
 msg=RegisterWindowMessageW(L"MeowScope.IME.UIThread.Context.1.3e92058f");
 if(!msg||!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_PIN,
       (LPCWSTR)(ULONG_PTR)uiHook,&module)){
  ReleaseSRWLockExclusive(&bootLock);return BOOT_HOOK_FAILED;
 }
 ++generation;if(!generation)++generation;
 ticket=generation;commandMessage=msg;targetWindow=hwnd;targetThread=tid;destroying=FALSE;lastResult=2;
 installerThread=GetCurrentThreadId();pinnedModule=module;
 hook=SetWindowsHookExW(WH_CALLWNDPROC,uiHook,module,tid);
 if(!hook){targetWindow=NULL;ReleaseSRWLockExclusive(&bootLock);return BOOT_HOOK_FAILED;}
 threadHook=hook;ReleaseSRWLockExclusive(&bootLock);
 if(!SendMessageTimeoutW(hwnd,msg,ticket,1,SMTO_ABORTIFHUNG|SMTO_BLOCK,1000,&ignored))return BOOT_TIMEOUT;
 AcquireSRWLockShared(&bootLock);
 pid=(generation==ticket)?lastResult:BOOT_STALE;
 ReleaseSRWLockShared(&bootLock);return pid;
}

/* Explicit stop runs Detach on the target UI thread before removing the hook.
 * Destroyed windows can only be released after successful UI-thread cleanup.
 */
__declspec(dllexport) DWORD WINAPI MeowImeBootstrapStop(void) {
 HWND hwnd;HHOOK hook;UINT msg;ULONG_PTR ticket;DWORD result;DWORD_PTR ignored;
 if(!TryAcquireSRWLockExclusive(&bootLock))return 12;
 hook=threadHook;hwnd=targetWindow;msg=commandMessage;ticket=generation;
 if(!hook){ReleaseSRWLockExclusive(&bootLock);return 0;}
 if(destroying&&hwnd){result=lastResult;ReleaseSRWLockExclusive(&bootLock);return result?result:12;}
 result=lastResult;if(hwnd)lastResult=2;ReleaseSRWLockExclusive(&bootLock);
 if(hwnd){
  if(!SendMessageTimeoutW(hwnd,msg,ticket,2,SMTO_ABORTIFHUNG|SMTO_BLOCK,1000,&ignored))return BOOT_TIMEOUT;
  AcquireSRWLockShared(&bootLock);result=lastResult;ReleaseSRWLockShared(&bootLock);
  if(result!=0)return result;
 }else if(result!=BOOT_DESTROYED)return result;
 AcquireSRWLockExclusive(&bootLock);
 if(threadHook!=hook||generation!=ticket){ReleaseSRWLockExclusive(&bootLock);return BOOT_STALE;}
 if(!UnhookWindowsHookEx(hook)){ReleaseSRWLockExclusive(&bootLock);return BOOT_HOOK_FAILED;}
 threadHook=NULL;targetWindow=NULL;targetThread=0;destroying=FALSE;lastResult=0;
 ReleaseSRWLockExclusive(&bootLock);return 0;
}

__declspec(dllexport) DWORD WINAPI MeowImeBootstrapStatus(void){
 DWORD result;AcquireSRWLockShared(&bootLock);result=lastResult;ReleaseSRWLockShared(&bootLock);return result;
}

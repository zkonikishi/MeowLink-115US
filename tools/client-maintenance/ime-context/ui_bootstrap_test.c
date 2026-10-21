#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <imm.h>
#include <assert.h>
#include <stdio.h>
DWORD WINAPI MeowImeBootstrap(HWND);
DWORD WINAPI MeowImeBootstrapStop(void);
DWORD WINAPI MeowImeBootstrapStatus(void);
static unsigned destroyCalls;
typedef struct {HWND hwnd;DWORD result;} StartCall;
static DWORD WINAPI startFromWorker(LPVOID arg){StartCall *call=(StartCall*)arg;call->result=MeowImeBootstrap(call->hwnd);return 0;}
static LRESULT CALLBACK testProc(HWND h,UINT m,WPARAM w,LPARAM l){
 if(m==WM_DESTROY){HIMC c=ImmGetContext(h);assert(!c);++destroyCalls;}
 return DefWindowProcW(h,m,w,l);
}
int main(void){
 WNDCLASSW wc={0};HWND hwnd;HIMC previous,current;DWORD result;
 wc.lpfnWndProc=testProc;wc.hInstance=GetModuleHandleW(NULL);wc.lpszClassName=L"MeowImeIsolatedFixture";
 assert(RegisterClassW(&wc));
 hwnd=CreateWindowExW(0,wc.lpszClassName,L"",WS_OVERLAPPED,0,0,20,20,NULL,NULL,wc.hInstance,NULL);assert(hwnd);
 previous=ImmAssociateContext(hwnd,NULL);
 {StartCall call={hwnd,99};MSG message;DWORD wait;HANDLE worker=CreateThread(NULL,0,startFromWorker,&call,0,NULL);assert(worker);
  for(;;){wait=MsgWaitForMultipleObjects(1,&worker,FALSE,5000,QS_ALLINPUT);if(wait==WAIT_OBJECT_0)break;
   assert(wait==WAIT_OBJECT_0+1);while(PeekMessageW(&message,NULL,0,0,PM_REMOVE)){TranslateMessage(&message);DispatchMessageW(&message);}}
  CloseHandle(worker);result=call.result;}
 printf("real HWND bootstrap result=%lu\n",result);
 assert(result==0);current=ImmGetContext(hwnd);assert(current);ImmReleaseContext(hwnd,current);
 assert(MeowImeBootstrap(hwnd)==12);
 assert(MeowImeBootstrapStop()==0);assert(!ImmGetContext(hwnd));
 assert(MeowImeBootstrap(hwnd)==0);
 assert(DestroyWindow(hwnd));assert(destroyCalls==1);
 assert(MeowImeBootstrapStatus()==22);assert(MeowImeBootstrapStop()==0);
 /* The original OS-owned default context was never destroyed by the plugin. */
 if(previous){hwnd=CreateWindowExW(0,wc.lpszClassName,L"",WS_OVERLAPPED,0,0,20,20,NULL,NULL,wc.hInstance,NULL);assert(hwnd);
  ImmAssociateContext(hwnd,previous);current=ImmGetContext(hwnd);assert(current==previous);ImmReleaseContext(hwnd,current);
  ImmAssociateContext(hwnd,NULL);assert(DestroyWindow(hwnd));}
 assert(UnregisterClassW(wc.lpszClassName,wc.hInstance));
 puts("PASS real hidden HWND/IMM/thread hook: attach, duplicate-start refusal, explicit stop, restart, detach before native WM_DESTROY, NCDESTROY cleanup; no game/no visible window.");
 return 0;
}

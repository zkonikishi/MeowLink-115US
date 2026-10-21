@echo off
cd /d "%~dp0"
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 exit /b 1
cl /nologo /LD /O2 /MT /W4 /WX /GS /utf-8 ime_context.c ui_bootstrap.c /Fe:MeowImeContext.dll /link user32.lib imm32.lib
if errorlevel 1 exit /b 1
cl /nologo /O2 /MT /W4 /WX /GS /utf-8 ime_context_test.c /Fe:ime_context_test.exe /link user32.lib imm32.lib
if errorlevel 1 exit /b 1
ime_context_test.exe
if errorlevel 1 exit /b 1
cl /nologo /O2 /MT /W4 /WX /GS /utf-8 ui_bootstrap_test.c /Fe:ui_bootstrap_test.exe /link MeowImeContext.lib user32.lib imm32.lib
if errorlevel 1 exit /b 1
ui_bootstrap_test.exe
exit /b %errorlevel%

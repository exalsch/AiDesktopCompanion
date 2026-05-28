@echo off
REM Launch `tauri dev` with the MSVC + LLVM/Clang environment set up so
REM whisper-rs-sys's bindgen can find <stdio.h> and the Windows SDK headers.
REM
REM Use this when local-stt builds fail with errors like:
REM   "fatal error: 'stdio.h' file not found"
REM   "Using bundled bindings.rs, which may be out of date"
REM   "evaluation of `_` failed here" (size-of overflow on _IO_FILE/_G_fpos64_t)
REM
REM Requires:
REM   - Visual Studio 2017+ or VS Build Tools with "Desktop development with C++"
REM   - LLVM/Clang (winget install -e --id LLVM.LLVM)
REM
REM Override LIBCLANG_PATH / CLANG_PATH if your LLVM lives somewhere other
REM than the default `C:\Program Files\LLVM\bin`.

setlocal

REM --- Locate Visual Studio via vswhere (standard install location since VS 2017) ---
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
  echo [dev-with-stt] vswhere not found at "%VSWHERE%"
  echo [dev-with-stt] Install Visual Studio 2017+ or VS Build Tools.
  exit /b 1
)

for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -property installationPath`) do set "VSINSTALL=%%i"
if not defined VSINSTALL (
  echo [dev-with-stt] No Visual Studio installation found by vswhere.
  exit /b 1
)

set "VCVARS=%VSINSTALL%\VC\Auxiliary\Build\vcvars64.bat"
if not exist "%VCVARS%" (
  echo [dev-with-stt] vcvars64.bat not found at "%VCVARS%"
  echo [dev-with-stt] Install the "Desktop development with C++" workload.
  exit /b 1
)

call "%VCVARS%" >nul
if errorlevel 1 (
  echo [dev-with-stt] vcvars64.bat failed
  exit /b 1
)

REM --- LLVM/Clang for bindgen. Override these env vars if your LLVM is elsewhere. ---
if not defined LIBCLANG_PATH set "LIBCLANG_PATH=C:\Program Files\LLVM\bin"
if not defined CLANG_PATH set "CLANG_PATH=C:\Program Files\LLVM\bin\clang.exe"
if not defined BINDGEN_EXTRA_CLANG_ARGS set "BINDGEN_EXTRA_CLANG_ARGS=--target=x86_64-pc-windows-msvc"

if not exist "%LIBCLANG_PATH%" (
  echo [dev-with-stt] LIBCLANG_PATH does not exist: "%LIBCLANG_PATH%"
  echo [dev-with-stt] Install LLVM: winget install -e --id LLVM.LLVM
  exit /b 1
)

REM --- Run from the app/ directory (this script lives in app/scripts/) ---
pushd "%~dp0.."
call npm run tauri -- dev
set "RC=%ERRORLEVEL%"
popd
exit /b %RC%

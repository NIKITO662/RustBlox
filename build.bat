@echo off
rem Developer build. Players should use the installer from the Releases page instead.
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. Get it from https://rustup.rs and run this again.
  pause & exit /b 1
)
where link.exe >nul 2>nul
if errorlevel 1 (
  echo.
  echo The Microsoft C++ linker was not found. Install "Build Tools for Visual Studio"
  echo from https://visualstudio.microsoft.com/downloads/ and tick
  echo "Desktop development with C++", then run this again.
  echo ^(Or skip all this: push to GitHub and let the workflow build the installer.^)
  echo.
  pause & exit /b 1
)
cargo build --release --locked
if errorlevel 1 ( echo Build failed. & pause & exit /b 1 )
echo.
echo Built target\release\rustblox.exe
where iscc >nul 2>nul
if not errorlevel 1 (
  iscc installer\setup.iss
  echo Installer is in the dist folder.
) else (
  echo Inno Setup not found, skipping the installer step.
)
pause

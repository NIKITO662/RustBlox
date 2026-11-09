; Inno Setup script for RustBlox. Build with:  iscc installer\setup.iss
; Expects the release build at ..\target\release\rustblox.exe
#define AppName "RustBlox"
#define AppVersion "0.2.0"
#define AppPublisher "ItsNiki66"
#define AppExe "RustBlox.exe"

[Setup]
AppId={{64FFC020-1927-41AD-AA17-AB1A9CE35C7B}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=RustBlox-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
ArchitecturesInstallIn64BitMode=x64compatible
VersionInfoVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"
Name: "setdefault"; Description: "Open Roblox links with {#AppName} (recommended)"; GroupDescription: "Roblox:"

[Files]
Source: "..\target\release\rustblox.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Parameters: "--register"; Flags: runhidden; Tasks: setdefault
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#AppExe}"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "Unregister"

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{localappdata}\RustBlox');
    if DirExists(DataDir) then
      if MsgBox('Also delete your RustBlox data (settings, mod packs, the downloaded Roblox client)?', mbConfirmation, MB_YESNO or MB_DEFAULT_NO) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;

; OneAsr Windows installer (Inno Setup 6)
; Single product package — CUDA engines in binary; CUDA DLLs downloaded in-app.
; Stage payload: scripts/pack-release.ps1 → dist\OneAsr\
;
;   iscc /DMyAppVersion=0.1.0 installer\OneAsr.iss

#define MyAppName "OneAsr"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "OneAsr"
#define MyAppExeName "oneasr.exe"
#define MyAppId "{{A7C3E2B1-4D5F-4A8E-9C1D-2E3F4A5B6C7D}"

[Setup]
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
; Models/CUDA download into {app} next to the exe, so the default must stay in
; the user profile even for elevated installs - C:\Program Files would make
; every model download fail with access denied (os error 5).
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\release
OutputBaseFilename=OneAsr_{#MyAppVersion}_setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
; Per-user only: the old dialog radio ("install for all users") sent {autopf}
; to C:\Program Files where the app cannot write models/ or dll/.
PrivilegesRequiredOverridesAllowed=commandline
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#MyAppExeName}
SetupIconFile=..\assets\icons\app-icon.ico
DisableDirPage=no
SetupLogging=yes

[Languages]
; Chinese first (vendored — not in stock Inno "Languages" install).
; English from the compiler install.
Name: "chinesesimplified"; MessagesFile: "languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\OneAsr\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Dirs]
Name: "{app}\models"; Flags: uninsalwaysuninstall
Name: "{app}\output"; Flags: uninsalwaysuninstall
Name: "{app}\runs"; Flags: uninsalwaysuninstall
Name: "{app}\dll"; Flags: uninsalwaysuninstall

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Keep models/output/runs for the user; only remove settings + optional CUDA components.
Type: files; Name: "{app}\settings.json"
Type: filesandordirs; Name: "{app}\dll"

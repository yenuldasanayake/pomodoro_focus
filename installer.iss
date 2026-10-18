; Pomodoro Focus — Inno Setup script
; Build with: "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer.iss

#define MyAppName "Pomodoro Focus"
#define MyAppVersion "1.0.1"
#define MyAppPublisher "Yenul"
#define MyAppExeName "pomodoro_focus.exe"
#define MyAppId "{{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}"

[Setup]
; Unique AppId — generate a fresh GUID in Inno Setup IDE (Tools → Generate GUID)
AppId={#MyAppId}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}

; Install to user profile by default — no admin required.
; This is what makes the data folder (settings.json, data\) writable by the app.
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes

; Icon shown on Setup.exe itself
SetupIconFile=icon.ico

; Icon shown in Apps & Features / Control Panel
UninstallDisplayIcon={app}\{#MyAppExeName}

; Output
OutputDir=Output
OutputBaseFilename=PomodoroFocus-Setup-{#MyAppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

; Allow running as normal user — no UAC prompt on install
PrivilegesRequired=lowest

; Setup detects the running app and asks to close it before installing.
; This works because Windows Restart Manager sees the exe file is in use.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startupicon"; Description: "Start Pomodoro Focus when I log in"; GroupDescription: "Startup:"; Flags: unchecked

[Files]
Source: "target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "icon.png"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Start Menu shortcut — WorkingDir must point to {app} so settings.json and data\ land there
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"; Comment: "Pomodoro Focus"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"

; Desktop shortcut (optional, from Tasks)
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

; Startup shortcut (optional, from Tasks)
Name: "{userstartup}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: startupicon

[Run]
; Offer to launch after install
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Do NOT delete the user's data on uninstall by default.
; If you want a clean uninstall that removes everything, uncomment these:
; Type: filesandordirs; Name: "{app}\data"
; Type: files; Name: "{app}\settings.json"
; Type: filesandordirs; Name: "{app}\exports"
#ifndef AppVersion
  #error AppVersion must be supplied with /DAppVersion=1.2.3
#endif
#ifndef CliBinary
  #define CliBinary "target\x86_64-pc-windows-msvc\release\napomni-cli.exe"
#endif
#ifndef AppBinary
  #define AppBinary "target\x86_64-pc-windows-msvc\release\napomni.exe"
#endif

[Setup]
AppId={{74C82C8F-740A-4676-9453-06EB556178CC}
AppName=Napomni
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Napomni
DefaultGroupName=Napomni
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
SourceDir={#SourcePath}..
OutputDir=dist
OutputBaseFilename=Napomni-{#AppVersion}-windows-x64-setup
SetupIconFile=assets\app.ico
UninstallDisplayIcon={app}\napomni.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#AppBinary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#CliBinary}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Napomni"; Filename: "{app}\napomni.exe"; Parameters: "--open"
Name: "{autodesktop}\Napomni"; Filename: "{app}\napomni.exe"; Parameters: "--open"; Tasks: desktopicon

[Run]
Filename: "{app}\napomni.exe"; Parameters: "--open"; Description: "{cm:LaunchProgram,Napomni}"; Flags: nowait postinstall skipifsilent

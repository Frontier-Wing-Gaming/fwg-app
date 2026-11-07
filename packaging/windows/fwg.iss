; Inno Setup script for the Windows installer. Built by .github/workflows/desktop-release.yml:
;   iscc /DAppVersion=<version> packaging\windows\fwg.iss
; Installs per user by default (no admin prompt); the first page offers "install for all users".

#ifndef AppVersion
  #error Pass the version: iscc /DAppVersion=<version>
#endif

[Setup]
AppId={{9F0D079F-1F90-4F41-8989-F5AB6FCA15CB}
AppName=Frontier Gaming Network
AppVersion={#AppVersion}
AppPublisher=Frontier Wing Gaming
AppPublisherURL=https://www.frontierwinggaming.network/
AppSupportURL=https://github.com/Frontier-Wing-Gaming/fwg-app
DefaultDirName={autopf}\Frontier Gaming Network
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=FrontierGamingNetwork-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\crates\fwg-app\icon.ico
UninstallDisplayIcon={app}\fwg.exe
UninstallDisplayName=Frontier Gaming Network
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\fwg.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Frontier Gaming Network"; Filename: "{app}\fwg.exe"
Name: "{autodesktop}\Frontier Gaming Network"; Filename: "{app}\fwg.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\fwg.exe"; Description: "{cm:LaunchProgram,Frontier Gaming Network}"; Flags: nowait postinstall skipifsilent

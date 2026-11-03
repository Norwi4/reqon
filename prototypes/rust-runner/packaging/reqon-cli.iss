#ifndef AppVersion
  #define AppVersion "0.14.1"
#endif
#ifndef AppId
  #define AppId "Reqon.CLI"
#endif
#ifndef EnvironmentKey
  #define EnvironmentKey "Environment"
#endif
#ifndef StateKey
  #define StateKey "Software\Reqon\CLI"
#endif

[Setup]
AppId={#AppId}
AppName=Reqon CLI
AppVersion={#AppVersion}
AppPublisher=Reqon
VersionInfoVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Reqon CLI
DisableDirPage=auto
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Reqon-CLI-Setup-{#AppVersion}-win64
SetupIconFile=..\..\..\Reqon_Icon_Pack_Approved\windows\reqon.ico
UninstallDisplayIcon={app}\reqon.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Messages]
english.FinishedLabel=Reqon CLI has been installed and is available as reqon from any folder.%n%nClose and reopen your terminal, then run:%nreqon --version%nreqon --help
russian.FinishedLabel=Reqon CLI установлен. Команда reqon доступна из любой папки.%n%nЗакройте и заново откройте терминал, затем выполните:%nreqon --version%nreqon --help

[Files]
Source: "..\dist\reqon.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\..\Reqon_Icon_Pack_Approved\windows\reqon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Code]
#include "user-path.iss"

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if Pos(';', ExpandConstant('{app}')) > 0 then
    Result := 'The installation folder cannot contain a semicolon (PATH separator).';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    InstallUserPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    UninstallUserPath(ExpandConstant('{app}'));
end;

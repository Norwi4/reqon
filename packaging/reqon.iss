#define AppName "Reqon"
#define AppVersion "0.19.0"
#define AppExe "Reqon.exe"

[Setup]
AppId={{D860FEA6-4908-4C53-BCA8-51662A1D759B}
AppName={#AppName}
AppVersion={#AppVersion}
VersionInfoVersion={#AppVersion}
VersionInfoProductVersion={#AppVersion}
VersionInfoProductName={#AppName}
AppPublisher=Reqon
DefaultDirName={code:GetReqonInstallDir}
DisableDirPage=no
UsePreviousAppDir=no
UsePreviousGroup=no
DefaultGroupName=Reqon
OutputDir=..\dist
OutputBaseFilename=Reqon-Setup-{#AppVersion}-win64
SetupIconFile=..\Reqon_Icon_Pack_Approved\windows\reqon.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\reqon-{#AppVersion}.ico
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\Reqon\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\Reqon_Icon_Pack_Approved\windows\reqon.ico"; DestDir: "{app}"; DestName: "reqon-{#AppVersion}.ico"; Flags: ignoreversion

[InstallDelete]
Type: files; Name: "{app}\Getman.exe"
Type: files; Name: "{autoprograms}\Getman.lnk"
Type: files; Name: "{autodesktop}\Getman.lnk"

[Icons]
Name: "{autoprograms}\Reqon"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\reqon-{#AppVersion}.ico"; AppUserModelID: "Norwi4.Reqon"
Name: "{autodesktop}\Reqon"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\reqon-{#AppVersion}.ico"; Tasks: desktopicon; AppUserModelID: "Norwi4.Reqon"

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,Reqon}"; Flags: nowait postinstall skipifsilent

[Code]
#include "reqon-paths.iss"

type
  TReqonShellFileInfo = record
    Icon: THandle;
    IconIndex: Integer;
    Attributes: Cardinal;
    DisplayName: array[0..259] of Char;
    TypeName: array[0..79] of Char;
  end;

function SHGetFileInfo(Path: string; Attributes: Cardinal; var Info: TReqonShellFileInfo; Size: Cardinal; Flags: Cardinal): THandle;
  external 'SHGetFileInfoW@shell32.dll stdcall';
procedure SHUpdateImage(Path: string; IconIndex: Integer; Flags: Cardinal; ImageIndex: Integer);
  external 'SHUpdateImageW@shell32.dll stdcall';
procedure SHChangeNotify(EventId: Integer; Flags: Cardinal; Item1: string; Item2: THandle);
  external 'SHChangeNotify@shell32.dll stdcall';

procedure CurStepChanged(CurStep: TSetupStep);
var
  Info: TReqonShellFileInfo;
  ExePath: string;
begin
  if CurStep = ssPostInstall then
  begin
    { Update only Reqon shell items. A versioned ICO avoids the cached EXE icon. }
    ExePath := ExpandConstant('{app}\{#AppExe}');
    if SHGetFileInfo(ExePath, 0, Info, SizeOf(Info), $00004000) <> 0 then
      SHUpdateImage(ExePath, 0, 0, Info.IconIndex);
    SHChangeNotify($00002000, $00001005, ExePath, 0);
    SHChangeNotify($00002000, $00001005, ExpandConstant('{autoprograms}\Reqon.lnk'), 0);
    if FileExists(ExpandConstant('{autodesktop}\Reqon.lnk')) then
      SHChangeNotify($00002000, $00001005, ExpandConstant('{autodesktop}\Reqon.lnk'), 0);
  end;
end;

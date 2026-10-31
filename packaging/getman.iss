#define AppName "Getman"
#define AppVersion "0.11.0"
#define AppExe "Getman.exe"

[Setup]
AppId={{D860FEA6-4908-4C53-BCA8-51662A1D759B}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Getman
DefaultDirName={localappdata}\Programs\Getman
DisableDirPage=no
UsePreviousAppDir=yes
DefaultGroupName=Getman
OutputDir=..\dist
OutputBaseFilename=Getman-Setup-{#AppVersion}-win64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\Getman\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Getman"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\Getman"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,Getman}"; Flags: nowait postinstall skipifsilent

#ifndef AppVersion
  #define AppVersion "0.23.1"
#endif
#ifndef AppGuid
  #define AppGuid "D860FEA6-4908-4C53-BCA8-51662A1D759B"
#endif
#ifndef BuildDist
  #define BuildDist "..\dist\windows-" + AppVersion + "\Reqon"
#endif
#ifndef OutputPath
  #define OutputPath "..\dist"
#endif
#ifndef OutputName
  #define OutputName "Reqon-Setup-" + AppVersion
#endif
[Setup]
AppId={{{#AppGuid}}
#ifdef TestInstall
AppName=Reqon Installer QA
#else
AppName=Reqon
#endif
AppVersion={#AppVersion}
VersionInfoVersion={#AppVersion}
VersionInfoProductVersion={#AppVersion}
VersionInfoProductName=Reqon
VersionInfoCompany=Reqon
VersionInfoDescription=Reqon Installer
AppPublisher=Reqon
AppPublisherURL=https://reqon.ru/
AppSupportURL=https://github.com/Norwi4/reqon/issues
AppUpdatesURL=https://github.com/Norwi4/reqon/releases/latest
DefaultDirName={code:GetReqonInstallDir}
DisableDirPage=no
UsePreviousAppDir=no
UsePreviousGroup=no
DefaultGroupName=Reqon
OutputDir={#OutputPath}
OutputBaseFilename={#OutputName}
SetupIconFile=..\assets\brand\circular-v1\windows\reqon.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern light windows11 includetitlebar hidebevels
WizardImageFile=branding\wizard-light.png
WizardImageBackColor=#F5F3EE
WizardSmallImageFile=..\assets\brand\circular-v1\png\reqon-256.png
DisableWelcomePage=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\reqon-{#AppVersion}.ico
CloseApplications=yes
RestartApplications=no
#ifdef TestInstall
AppMutex=Norwi4.Reqon.InstallerQA
#else
AppMutex=Norwi4.Reqon
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Messages]
english.WelcomeLabel1=Welcome to Reqon
english.WelcomeLabel2=Your workspace for building, sending and testing API requests.%n%nThis will install [name/ver] on your computer.%n%nDeveloper: Reqon%nWebsite: reqon.ru%n%nClick Next to continue.
english.FinishedHeadingLabel=Reqon is ready
english.FinishedLabel=You're ready to start working with APIs.%n%nClick Finish to close Setup.
english.FinishedLabelNoIcons=You're ready to start working with APIs.%n%nClick Finish to close Setup.
english.BeveledLabel=Reqon  /  reqon.ru
russian.WelcomeLabel1=Добро пожаловать в Reqon
russian.WelcomeLabel2=Ваше рабочее пространство для создания, отправки и тестирования API-запросов.%n%nНа компьютер будет установлен [name/ver].%n%nРазработчик: Reqon%nСайт: reqon.ru%n%nНажмите «Далее», чтобы продолжить.
russian.FinishedHeadingLabel=Reqon готов к работе
russian.FinishedLabel=Всё готово для работы с API.%n%nНажмите «Завершить», чтобы закрыть установщик.
russian.FinishedLabelNoIcons=Всё готово для работы с API.%n%nНажмите «Завершить», чтобы закрыть установщик.
russian.BeveledLabel=Reqon  /  reqon.ru

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BuildDist}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\assets\brand\circular-v1\windows\reqon.ico"; DestDir: "{app}"; DestName: "reqon-{#AppVersion}.ico"; Flags: ignoreversion

#ifndef TestInstall
[Icons]
Name: "{autoprograms}\Reqon"; Filename: "{app}\Reqon.exe"; IconFilename: "{app}\reqon-{#AppVersion}.ico"; AppUserModelID: "Norwi4.Reqon"
Name: "{autodesktop}\Reqon"; Filename: "{app}\Reqon.exe"; IconFilename: "{app}\reqon-{#AppVersion}.ico"; Tasks: desktopicon; AppUserModelID: "Norwi4.Reqon"
#endif

[Run]
Filename: "{app}\Reqon.exe"; Description: "{cm:LaunchProgram,Reqon}"; Flags: nowait postinstall skipifsilent

[Code]
function PreviousInstallDir: string;
begin
  Result := '';
  RegQueryStringValue(HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#AppGuid}}_is1',
    'Inno Setup: App Path', Result);
end;

function GetReqonInstallDir(Param: string): string;
begin
  Result := RemoveBackslashUnlessRoot(PreviousInstallDir);
  if Result = '' then
#ifdef TestInstall
    Result := ExpandConstant('{localappdata}\Programs\ReqonInstallerQA');
#else
    Result := ExpandConstant('{localappdata}\Programs\Reqon');
#endif
end;

procedure RegisterExtraCloseApplicationsResources;
var PreviousDir: string;
begin
  PreviousDir := PreviousInstallDir;
  if PreviousDir <> '' then begin
    RegisterExtraCloseApplicationsResource(AddBackslash(PreviousDir) + 'Reqon.exe');
    RegisterExtraCloseApplicationsResource(AddBackslash(PreviousDir) + 'reqon-cli.exe');
    RegisterExtraCloseApplicationsResource(AddBackslash(PreviousDir) + 'Getman.exe');
  end;
end;

{ Profile data is deliberately absent from Files/InstallDelete/UninstallDelete.
  It lives in APPDATA\Reqon; conversion runs once on first application launch. }

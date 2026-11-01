; Compile/run a read-only installer path check. InitializeSetup aborts before installing.
[Setup]
AppName=Reqon path checks
AppVersion=1
DefaultDirName={tmp}\Reqon-path-check
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
OutputDir=..\build
OutputBaseFilename=reqon-installer-path-check

[Code]
#include "..\packaging\reqon-paths.iss"

procedure CheckPath(Previous, Expected: string);
begin
  if ReqonInstallDir(Previous) <> Expected then
    RaiseException('Unexpected installer directory for ' + Previous);
end;

function InitializeSetup: Boolean;
begin
  CheckPath('', ExpandConstant('{localappdata}\Programs\Reqon'));
  CheckPath('C:\Users\test\AppData\Local\Programs\Getman\', 'C:\Users\test\AppData\Local\Programs\Reqon');
  CheckPath('D:\Tools\getman', 'D:\Tools\Reqon');
  CheckPath('D:\Tools\Reqon', 'D:\Tools\Reqon');
  CheckPath('D:\API Tools', 'D:\API Tools');
  CheckPath('D:\', 'D:\');
  Log('REQON_PATH_CHECKS_PASSED; suggested directory: ' + GetReqonInstallDir(''));
  Result := False;
end;

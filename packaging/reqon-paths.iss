{ Preserve user-selected folders, but migrate the former product folder name. }
function ReqonInstallDir(PreviousDir: string): string;
begin
  Result := RemoveBackslashUnlessRoot(PreviousDir);
  if Result = '' then
    Result := ExpandConstant('{localappdata}\Programs\Reqon')
  else if CompareText(ExtractFileName(Result), 'Getman') = 0 then
    Result := AddBackslash(ExtractFileDir(Result)) + 'Reqon';
end;

function PreviousInstallDir: string;
begin
  Result := '';
  RegQueryStringValue(HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{D860FEA6-4908-4C53-BCA8-51662A1D759B}_is1',
    'Inno Setup: App Path', Result);
end;

function GetReqonInstallDir(Param: string): string;
begin
  Result := ReqonInstallDir(PreviousInstallDir);
end;

procedure RegisterExtraCloseApplicationsResources;
var
  PreviousDir: string;
begin
  PreviousDir := PreviousInstallDir;
  if PreviousDir <> '' then
  begin
    RegisterExtraCloseApplicationsResource(AddBackslash(PreviousDir) + 'Reqon.exe');
    RegisterExtraCloseApplicationsResource(AddBackslash(PreviousDir) + 'Getman.exe');
  end;
end;

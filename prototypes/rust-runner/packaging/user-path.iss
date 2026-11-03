{ Edit only the current user's PATH. Remember ownership so uninstall preserves
  entries that existed before installation. Registry reads remain unexpanded. }
const
  UserEnvironmentKey = '{#EnvironmentKey}';
  CliStateKey = '{#StateKey}';

function NormalizedDirectory(Value: String): String;
var
  Start, Finish: Integer;
  Name, Expanded, Remaining: String;
begin
  Remaining := Trim(Value);
  Result := '';
  while Remaining <> '' do
  begin
    Start := Pos('%', Remaining);
    if Start = 0 then
    begin
      Result := Result + Remaining;
      Break;
    end;
    Result := Result + Copy(Remaining, 1, Start - 1);
    Delete(Remaining, 1, Start);
    Finish := Pos('%', Remaining);
    if Finish = 0 then
    begin
      Result := Result + '%' + Remaining;
      Break;
    end;
    Name := Copy(Remaining, 1, Finish - 1);
    Expanded := GetEnv(Name);
    if Expanded = '' then
      Expanded := '%' + Name + '%';
    Result := Result + Expanded;
    Delete(Remaining, 1, Finish);
  end;
  Result := Trim(Result);
  if (Length(Result) >= 2) and (Result[1] = '"') and
     (Result[Length(Result)] = '"') then
    Result := Copy(Result, 2, Length(Result) - 2);
  StringChangeEx(Result, '/', '\', True);
  Result := RemoveBackslashUnlessRoot(Result);
end;

function SameDirectory(Left, Right: String): Boolean;
begin
  Result := CompareText(NormalizedDirectory(Left), NormalizedDirectory(Right)) = 0;
end;

function FilterPath(Value, Directory: String; var Found: Boolean): String;
var
  Separator: Integer;
  Entry: String;
  First: Boolean;
begin
  Result := '';
  Found := False;
  First := True;
  repeat
    Separator := Pos(';', Value);
    if Separator = 0 then
      Entry := Value
    else
      Entry := Copy(Value, 1, Separator - 1);
    if SameDirectory(Entry, Directory) then
      Found := True
    else
    begin
      if not First then
        Result := Result + ';';
      Result := Result + Entry;
      First := False;
    end;
    if Separator = 0 then
      Break;
    Delete(Value, 1, Separator);
  until False;
end;

function ReadUserPath: String;
begin
  Result := '';
  if RegValueExists(HKCU, UserEnvironmentKey, 'Path') and
     not RegQueryStringValue(HKCU, UserEnvironmentKey, 'Path', Result) then
    RaiseException('Could not read the user PATH. No PATH changes were made.');
end;

procedure WriteUserPath(Value: String);
begin
  { RegWriteStringValue preserves REG_EXPAND_SZ when it already exists. }
  if not RegWriteStringValue(HKCU, UserEnvironmentKey, 'Path', Value) then
    RaiseException('Could not update the user PATH.');
end;

procedure InstallUserPath(Directory: String);
var
  Original, Updated, Owned, Ignored: String;
  Found: Boolean;
begin
  Original := ReadUserPath;
  Updated := Original;
  Owned := '';
  RegQueryStringValue(HKCU, CliStateKey, 'PathEntry', Owned);
  if (Owned <> '') and not SameDirectory(Owned, Directory) then
    Updated := FilterPath(Updated, Owned, Found);
  Ignored := FilterPath(Updated, Directory, Found);
  if not Found then
  begin
    if Updated <> '' then
      Updated := Updated + ';';
    Updated := Updated + Directory;
    WriteUserPath(Updated);
    if not RegWriteStringValue(HKCU, CliStateKey, 'PathEntry', Directory) then
    begin
      WriteUserPath(Original);
      RaiseException('Could not record CLI PATH ownership. PATH was restored.');
    end;
  end
  else if (Owned <> '') and not SameDirectory(Owned, Directory) then
  begin
    WriteUserPath(Updated);
    RegDeleteValue(HKCU, CliStateKey, 'PathEntry');
  end;
end;

procedure UninstallUserPath(Directory: String);
var
  Owned, Original, Updated: String;
  Found: Boolean;
begin
  Owned := '';
  if not RegQueryStringValue(HKCU, CliStateKey, 'PathEntry', Owned) or
     not SameDirectory(Owned, Directory) then
    Exit;
  Original := ReadUserPath;
  Updated := FilterPath(Original, Directory, Found);
  if Found then
    WriteUserPath(Updated);
  RegDeleteValue(HKCU, CliStateKey, 'PathEntry');
  RegDeleteKeyIfEmpty(HKCU, CliStateKey);
end;

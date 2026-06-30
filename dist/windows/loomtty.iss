; Inno Setup script for loomtty (Windows).
;
; Produces a single loomtty-<version>-x86_64-setup.exe that lets the user choose
; between a per-user install (no admin, %LOCALAPPDATA%\Programs) and an all-users
; install (Program Files, requires elevation). Adds loomtty to PATH for the
; chosen scope, creates Start Menu / optional desktop shortcuts, registers an
; uninstaller, and stops the background daemon on uninstall.
;
; Build:
;   - locally:  dist\windows\build-installer.ps1   (wraps ISCC with the defines)
;   - manually: ISCC.exe /DMyAppVersion=0.1.0 /DBinDir=..\..\target\release dist\windows\loomtty.iss
;
; Overridable defines (all have sensible defaults for a local release build):
;   MyAppVersion  version string shown in Add/Remove Programs       (default 0.1.0)
;   BinDir        directory holding loomtty.exe / loomtty-server.exe (default ..\..\target\release)
;   SourceRoot    repository root, for assets/LICENSE/shell scripts  (default ..\..)
;   OutputDir     where the setup .exe is written                    (default {SourceRoot}\target\installer)

#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#ifndef SourceRoot
  #define SourceRoot "..\.."
#endif
#ifndef BinDir
  #define BinDir SourceRoot + "\target\release"
#endif
#ifndef OutputDir
  #define OutputDir SourceRoot + "\target\installer"
#endif

#define MyAppName "loomtty"
#define MyAppPublisher "loomtty contributors"
#define MyAppURL "https://github.com/l1nxy/loomtty"
#define MyAppExeName "loomtty.exe"
; The GUI-subsystem launcher: shortcuts point here so opening loomtty from the
; Start Menu / desktop never flashes a console window. The CLI stays on
; loomtty.exe (console subsystem).
#define MyGuiExeName "loomtty-gui.exe"
#define MyServerExeName "loomtty-server.exe"

[Setup]
; A stable AppId keeps upgrades / uninstall entries consistent across versions.
AppId={{B3E7B2A4-9C1D-4F8E-A6B5-7D2C3E1F9A04}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
VersionInfoVersion={#MyAppVersion}

; Default to a per-user install (no admin). The dialog lets the user opt into
; an all-users install if they can elevate; `commandline` also honours the
; /ALLUSERS and /CURRENTUSER switches for silent/scripted installs.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline

; {autopf} -> Program Files for all-users, %LOCALAPPDATA%\Programs for per-user.
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
AllowNoIcons=yes
LicenseFile={#SourceRoot}\LICENSE

; x86_64 only (matches the release build target). The legacy "x64" identifier
; is used over "x64compatible" for compatibility with the older Inno Setup that
; may be preinstalled on CI runners; newer compilers map it to "x64os" with a
; (harmless) deprecation warning.
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
MinVersion=10.0

Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#SourceRoot}\assets\icons\icon.ico
UninstallDisplayIcon={app}\{#MyGuiExeName}
ChangesEnvironment=yes

OutputDir={#OutputDir}
OutputBaseFilename=loomtty-{#MyAppVersion}-x86_64-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add loomtty to the {code:PathScopeLabel} PATH (recommended)"; GroupDescription: "Environment:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\{#MyGuiExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\{#MyServerExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceRoot}\crates\loom-server\shell-integration\*"; DestDir: "{app}\shell-integration"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourceRoot}\assets\icons\icon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceRoot}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#SourceRoot}\README.md"; DestDir: "{app}"; DestName: "README.md"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyGuiExeName}"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyGuiExeName}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
; Launch via Explorer rather than running the GUI directly. Setup.exe runs with
; the "Redirection Guard" process mitigation (EnforceRedirectionTrust) that
; Windows applies to installer-class processes; that policy is INHERITED by any
; child we spawn here — including the daemon and, transitively, the shells it
; opens in its pseudo-consoles. With it active, CreateProcess fails with
; ERROR_UNTRUSTED_MOUNT_POINT (448) for any target reached through an untrusted
; junction/reparse point — e.g. a scoop-shimmed shell at
; ...\scoop\apps\<tool>\current\<tool>.exe (`current` is a junction). The first
; pane's shell then can't start, the only pane closes, and the window vanishes
; ("won't open"). Bouncing through Explorer.exe (the shell, unmitigated) starts
; loomtty in a fresh process tree without the inherited policy, matching a normal
; Start-Menu launch. Verified: Explorer-launched daemon reports
; EnforceRedirectionTrust=False and junction-pathed shells spawn fine.
Filename: "{win}\explorer.exe"; Parameters: """{app}\{#MyGuiExeName}"""; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

; NOTE: We intentionally do NOT run `loomtty kill-server` on uninstall.
; The control connection has no read timeout on Windows (named-pipe reads
; block indefinitely), so a wedged daemon would hang the silent uninstaller
; with no window to cancel. The pipe name is also machine-global, so an
; all-users uninstall could kill another logged-in user's session. Inno's
; built-in "file in use" handling schedules any locked exe for deletion on
; reboot, which is the safe behaviour.

[Code]
{ ── PATH management ───────────────────────────────────────────────────
  Per-user installs touch HKCU\Environment; all-users installs touch the
  system environment under HKLM. The directory is appended on install (when
  the addtopath task is selected) and removed on uninstall. ChangesEnvironment
  makes Setup broadcast WM_SETTINGCHANGE so new shells see the update. }

const
  SystemEnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';
  UserEnvKey = 'Environment';
  { Inno derives its uninstall registry key from the AppId by appending _is1. }
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{B3E7B2A4-9C1D-4F8E-A6B5-7D2C3E1F9A04}_is1';
  { The Windows per-process environment block is bounded; stay well under it so
    we never silently produce an over-length, unusable PATH. }
  MaxPathLen = 8000;

function PathRootKey: Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function PathSubKey: String;
begin
  if IsAdminInstallMode then
    Result := SystemEnvKey
  else
    Result := UserEnvKey;
end;

{ Shown in the "Add loomtty to the … PATH" task label. }
function PathScopeLabel(Param: String): String;
begin
  if IsAdminInstallMode then
    Result := 'system'
  else
    Result := 'user';
end;

procedure EnvAddPathIn(RootKey: Integer; SubKey, Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(RootKey, SubKey, 'Path', Paths) then
    Paths := '';

  { Already present (case-insensitive, surrounded by separators)? Done. }
  if Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0 then
    exit;

  { Refuse to grow PATH past the limit — a truncated PATH is worse than no
    PATH entry, and Windows truncates silently. }
  if Length(Paths) + Length(Dir) + 1 > MaxPathLen then begin
    MsgBox('Could not add loomtty to PATH: the ' + PathScopeLabel('') +
      ' PATH is already near the Windows length limit.' + #13#10 +
      'Add this folder to PATH manually:' + #13#10#13#10 + Dir, mbError, MB_OK);
    exit;
  end;

  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  Paths := Paths + Dir;

  { Write back as REG_EXPAND_SZ so existing %VAR% entries keep expanding. }
  if RegWriteExpandStringValue(RootKey, SubKey, 'Path', Paths) then
    Log('Added to PATH: ' + Dir)
  else
    Log('Failed to add to PATH: ' + Dir);
end;

procedure EnvRemovePathIn(RootKey: Integer; SubKey, Dir: String);
var
  Paths: String;
  P, RealStart: Integer;
begin
  if not RegQueryStringValue(RootKey, SubKey, 'Path', Paths) then
    exit;

  P := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';');
  if P = 0 then
    exit;

  { P indexes the synthetic ';' + Paths + ';' string, so the directory begins
    at real position P-1. For the FIRST entry (P=1) there is no preceding
    separator, so drop Dir plus the following ';'; otherwise drop the preceding
    ';' plus Dir. (The old `Delete(Paths, P-1, …)` passed index 0 for the first
    entry, whose behaviour is undefined and could corrupt the next entry.) }
  RealStart := P - 1;
  if RealStart < 1 then
    Delete(Paths, 1, Length(Dir) + 1)
  else
    Delete(Paths, RealStart, Length(Dir) + 1);

  if RegWriteExpandStringValue(RootKey, SubKey, 'Path', Paths) then
    Log('Removed from PATH: ' + Dir)
  else
    Log('Failed to remove from PATH: ' + Dir);
end;

{ Warn if loomtty is already installed in the *other* scope — installing again
  would create a second independent copy (its own PATH entry and shortcuts). }
function InitializeSetup(): Boolean;
var
  OtherRoot: Integer;
  OtherLabel: String;
begin
  Result := True;
  if IsAdminInstallMode then begin
    OtherRoot := HKEY_CURRENT_USER;
    OtherLabel := 'just for you (per-user)';
  end else begin
    OtherRoot := HKEY_LOCAL_MACHINE;
    OtherLabel := 'for all users';
  end;
  if RegKeyExists(OtherRoot, UninstallKey) then begin
    if MsgBox('loomtty is already installed ' + OtherLabel + '.' + #13#10#13#10 +
        'Installing it in a different scope creates a second, independent copy ' +
        'with its own PATH entry and Start Menu shortcut. Uninstalling the ' +
        'existing copy first is recommended.' + #13#10#13#10 + 'Continue anyway?',
        mbConfirmation, MB_YESNO) = IDNO then
      Result := False;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    EnvAddPathIn(PathRootKey, PathSubKey, ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then begin
    { Always clean the user PATH (where a per-user install added it — even if
      this uninstaller happens to run elevated), and the system PATH when
      elevated (where an all-users install added it). Robust regardless of the
      privilege mode the uninstaller runs in. }
    EnvRemovePathIn(HKEY_CURRENT_USER, UserEnvKey, ExpandConstant('{app}'));
    if IsAdminInstallMode then
      EnvRemovePathIn(HKEY_LOCAL_MACHINE, SystemEnvKey, ExpandConstant('{app}'));
  end;
end;

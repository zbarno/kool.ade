#define AppVersion GetEnv("KOOLADE_VERSION")
#define RepoRoot GetEnv("KOOLADE_REPO_ROOT")

[Setup]
AppId={{5C239B40-93C1-4AE2-AF50-24ACB719D27D}
AppName=Kool.ad/e
AppVersion={#AppVersion}
AppPublisher=Kool.ad/e
DefaultDirName={localappdata}\Programs\Koolade
DefaultGroupName=Kool.ad/e
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64
DisableProgramGroupPage=yes
OutputDir={#RepoRoot}\dist\native
OutputBaseFilename=koolade-{#AppVersion}-windows-x64-unsigned-ci
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#RepoRoot}\assets\brand\app-icon.ico
UninstallDisplayIcon={app}\koolade.ico

[Files]
Source: "{#RepoRoot}\target\release\koolade.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\assets\brand\app-icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Kool.ad/e"; Filename: "{app}\koolade.exe"; IconFilename: "{app}\koolade.ico"
Name: "{autodesktop}\Kool.ad/e"; Filename: "{app}\koolade.exe"; IconFilename: "{app}\koolade.ico"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

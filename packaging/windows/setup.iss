; Inno Setup 6.4+; both target architectures are compiled on an x64 build host.
#if Ver < EncodeVer(6, 4, 0) || Ver >= EncodeVer(7, 0, 0)
  #error This installer requires Inno Setup 6.4 or later in the 6.x series.
#endif

[Setup]
AppId=io.github.bandori-pet-lite.{#AppArch}
AppName=BANDORI-PET-LITE
AppVersion={#AppVersion}
AppPublisher=BANDORI-PET-LITE contributors
DefaultDirName={localappdata}\Programs\BANDORI-PET-LITE-{#AppArch}
DefaultGroupName=BANDORI-PET-LITE
PrivilegesRequired=lowest
DisableDirPage=yes
DisableProgramGroupPage=yes
#if AppArch == "aarch64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0
UninstallDisplayIcon={app}\bandori-pet.exe
SetupIconFile={#SourceDir}\assets\icons\bandori.ico
LicenseFile={#SourceDir}\LICENSE
OutputDir={#OutputDir}
OutputBaseFilename={#PackageName}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\BANDORI-PET-LITE ({#AppArch})"; Filename: "{app}\bandori-pet.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\bandori-pet.exe"; Description: "Launch BANDORI-PET-LITE"; Flags: nowait postinstall skipifsilent

; No recursive uninstall rule: user-created config, models and logs must survive.

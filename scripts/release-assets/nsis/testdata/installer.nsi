; A small installer laid out where `Vet` reads like Tauri's NSIS template lays out the real one
; (tauri-bundler's `installer.nsi`): Unicode, one solid LZMA stream, the stub's version resource,
; the app packed as a file, and the HKCU uninstall entry. `gen.sh` builds it from `payload.nsi`'s
; stand-in app. It never needs to run: the vet reads it.
Unicode true
SetCompressor /SOLID lzma
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
OutFile "${OUT}"
Name "${PRODUCTNAME}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\${PRODUCTNAME}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "LegalCopyright" "test"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
Section
  SetOutPath $INSTDIR
  File "${PAYLOAD}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
SectionEnd
Section Uninstall
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd

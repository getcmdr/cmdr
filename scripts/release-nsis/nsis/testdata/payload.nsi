; The stand-in for the installed `Cmdr.exe`: any small PE whose version resource says what Tauri's
; build stamps into the app (ProductName, CompanyName, ProductVersion). `gen.sh` builds it, then
; `installer.nsi` packs it the way Tauri's NSIS template packs the real one.
Unicode true
SetCompressor /SOLID lzma
OutFile "${OUT}"
Name "Cmdr"
RequestExecutionLevel user
SilentInstall silent
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Cmdr"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "Cmdr"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "test"
Section
SectionEnd

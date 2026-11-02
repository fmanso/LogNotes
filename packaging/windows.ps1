# Package the Windows build: the executable, the Qt libraries and plugins it needs, and the licence.
#
# Run by the `windows` job in .github/workflows/ci.yml, after `cargo build --release`. Not usable from the
# Linux side: windeployqt ships with the Windows Qt.

$ErrorActionPreference = "Stop"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$stage = "dist/stage/LogNotes-$version"

Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item target/release/lognotes-qt.exe "$stage/LogNotes.exe"
Copy-Item packaging/lognotes.svg, README.md, LICENSE $stage/

# Everything the executable loads at runtime: Qt DLLs, the QML modules, the platform and image plugins.
windeployqt --release --no-translations --no-system-d3d-compiler --no-opengl-sw "$stage/LogNotes.exe"

$zip = "dist/LogNotes-$version-windows-x86_64.zip"
Compress-Archive -Path "$stage/*" -DestinationPath $zip
Write-Host "wrote $zip"

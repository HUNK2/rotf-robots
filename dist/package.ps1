# Packs a release from an already built `cargo build --release --locked -p bumblebee -p rotf-export`:
#   work\release\rotf-robots-<version>-windows-x64.zip   the two programs, README, licence, notices
#   work\release\rotf_import-<version>.zip               the Blender add-on (manifest at the top)
#   work\release\SHA256SUMS.txt
# Needs cargo-about (`cargo install cargo-about --locked --features cli`). Run from anywhere:
#   powershell -ExecutionPolicy Bypass -File dist\package.ps1 [-Version 0.2.0]
param([string]$Version)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Version) {
    $Version = (Select-String -Path "$root\game\Cargo.toml" -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
}
$out = Join-Path $root 'work\release'
$name = "rotf-robots-$Version-windows-x64"
$stage = Join-Path $out $name
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $stage | Out-Null

foreach ($exe in 'bumblebee.exe', 'rotf-export.exe') {
    $built = Join-Path $root "target\release\$exe"
    if (-not (Test-Path $built)) { throw "$built is missing: build the release first" }
    Copy-Item $built $stage
}
Copy-Item "$root\dist\README.txt" $stage
Copy-Item "$root\LICENSE" "$stage\LICENSE.txt"
Copy-Item "$root\dist\FiraMono-LICENSE.txt" $stage
& cargo about generate --manifest-path "$root\Cargo.toml" --workspace -c "$root\dist\about.toml" "$root\dist\about.hbs" -o "$stage\THIRD-PARTY-NOTICES.html"
if ($LASTEXITCODE -ne 0) { throw 'cargo about failed' }

# Nothing from the game may ride along: only these files, by name.
$allowed = 'bumblebee.exe', 'rotf-export.exe', 'README.txt', 'LICENSE.txt', 'FiraMono-LICENSE.txt', 'THIRD-PARTY-NOTICES.html'
$extra = Get-ChildItem $stage | Where-Object { $allowed -notcontains $_.Name }
if ($extra) { throw "unexpected files in the release: $($extra.Name -join ', ')" }

Compress-Archive -Path $stage -DestinationPath "$out\$name.zip"
$addon = Join-Path $out "rotf_import-$Version.zip"
Compress-Archive -Path "$root\blender\rotf_import\__init__.py", "$root\blender\rotf_import\blender_manifest.toml" -DestinationPath $addon

$sums = Get-ChildItem $out -Filter *.zip | ForEach-Object {
    "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"
}
$sums | Set-Content -Encoding ascii "$out\SHA256SUMS.txt"
$sums

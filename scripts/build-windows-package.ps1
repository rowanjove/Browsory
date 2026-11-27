$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Utility/Microsoft.PowerShell.Utility.psd1') -Force -ErrorAction Stop
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tauriConfig = Get-Content -LiteralPath (Join-Path $workspace 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$packageConfig = Get-Content -LiteralPath (Join-Path $workspace 'package.json') -Raw | ConvertFrom-Json
$cargoManifest = Get-Content -LiteralPath (Join-Path $workspace 'src-tauri/Cargo.toml') -Raw
$version = [string]$tauriConfig.version
$cargoVersionMatch = [regex]::Match($cargoManifest, '(?m)^version = "([^"]+)"\r?$')
if ($version -ne $packageConfig.version -or -not $cargoVersionMatch.Success -or $version -ne $cargoVersionMatch.Groups[1].Value) {
    throw 'package.json, Cargo.toml and tauri.conf.json versions must match'
}

$oldStaticRuntime = $env:STATIC_VCRUNTIME
$oldEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$oldRustFlags = $env:RUSTFLAGS
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$pathRemapFlags = @(
    "--remap-path-prefix=$workspace=."
    "--remap-path-prefix=$(Join-Path $cargoHome 'registry/src')=.cargo/registry/src"
)
$flagSeparator = [string][char]31
if ($oldEncodedRustFlags) {
    $rustFlags = @($oldEncodedRustFlags.Split([char]31)) + $pathRemapFlags
} elseif ($oldRustFlags) {
    $rustFlags = @($oldRustFlags.Split([char]' ', [StringSplitOptions]::RemoveEmptyEntries)) + $pathRemapFlags
} else {
    $rustFlags = $pathRemapFlags
}
$env:CARGO_ENCODED_RUSTFLAGS = $rustFlags -join $flagSeparator
$env:STATIC_VCRUNTIME = 'true'
Push-Location $workspace
try {
    & pnpm tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "Tauri packaging failed with exit code $LASTEXITCODE" }

    $exe = Join-Path $workspace 'src-tauri/target/release/browsory.exe'
    & (Join-Path $PSScriptRoot 'verify-openssl-paths.ps1') -Executable $exe
    $delivery = Join-Path $workspace ('release/security-acceptance/package-' + [guid]::NewGuid().ToString())
    $portable = Join-Path $delivery 'portable'
    New-Item -ItemType Directory -Path $portable -Force | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $portable 'Browsory.exe')
    [IO.File]::WriteAllText((Join-Path $portable 'portable.txt'), 'portable')
    [IO.File]::WriteAllText(
        (Join-Path $portable 'README.txt'),
        "Browsory $version portable build. Keep portable.txt beside Browsory.exe; app data is stored in the sibling data folder. The database vault passphrase and recovery key are separate from the optional screen PIN. Microsoft Edge WebView2 Runtime is required. This package contains no user data or credentials. File deletion does not guarantee physical erasure."
    )

    $licenseDirectory = Join-Path $portable 'licenses'
    New-Item -ItemType Directory -Path $licenseDirectory -Force | Out-Null
    $licenseFiles = @(
        @{ Source = (Join-Path $workspace 'LICENSE'); Destination = 'LICENSE' },
        @{ Source = (Join-Path $workspace 'NOTICE'); Destination = 'NOTICE' },
        @{ Source = (Join-Path $workspace 'docs/THIRD_PARTY_NOTICES.md'); Destination = 'THIRD_PARTY_NOTICES.md' },
        @{ Source = (Join-Path $workspace 'src-tauri/openssl/LICENSE.txt'); Destination = 'OpenSSL-LICENSE.txt' },
        @{ Source = (Join-Path $workspace 'docs/licenses/sqlcipher-src/LICENSE'); Destination = 'SQLCipher-LICENSE' },
        @{ Source = (Join-Path $workspace 'docs/licenses/sqlcipher-src/LICENSE-sqlcipher'); Destination = 'LICENSE-sqlcipher' },
        @{ Source = (Join-Path $workspace 'docs/licenses/sqlcipher-src/LICENSE-libtomcrypt'); Destination = 'LICENSE-libtomcrypt' },
        @{ Source = (Join-Path $workspace 'docs/licenses/webpki-roots/LICENSE'); Destination = 'webpki-roots-LICENSE.txt' }
    )
    foreach ($license in $licenseFiles) {
        if (-not (Test-Path -LiteralPath $license.Source -PathType Leaf)) {
            throw "Missing redistribution license: $($license.Source)"
        }
        Copy-Item -LiteralPath $license.Source -Destination (Join-Path $licenseDirectory $license.Destination)
        $sourceHash = (Get-FileHash -LiteralPath $license.Source -Algorithm SHA256).Hash
        $copyHash = (Get-FileHash -LiteralPath (Join-Path $licenseDirectory $license.Destination) -Algorithm SHA256).Hash
        if ($sourceHash -ne $copyHash) { throw "License copy validation failed: $($license.Destination)" }
    }

    $portableZip = Join-Path $delivery "Browsory_${version}_x64_portable.zip"
    Compress-Archive -Path (Join-Path $portable '*') -DestinationPath $portableZip
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($portableZip)
    try {
        $entries = @($zip.Entries | ForEach-Object { $_.FullName.Replace('\', '/') })
        foreach ($license in $licenseFiles) {
            $expectedEntry = 'licenses/' + $license.Destination
            if ($expectedEntry -notin $entries) { throw "Portable package is missing $expectedEntry" }
        }
    }
    finally { $zip.Dispose() }
    $validation = & (Join-Path $PSScriptRoot 'verify-windows-package.ps1') -Executable $exe -RequireStaticRuntime -PortableZip $portableZip

    $installer = Join-Path $delivery "Browsory_${version}_x64-setup.exe"
    $builtInstaller = Join-Path $workspace "src-tauri/target/release/bundle/nsis/Browsory_${version}_x64-setup.exe"
    Copy-Item -LiteralPath $builtInstaller -Destination $installer
    & (Join-Path $PSScriptRoot 'verify-openssl-paths.ps1') -Executable $exe -Installer $installer
    $validation = & (Join-Path $PSScriptRoot 'verify-windows-package.ps1') -Executable $exe -RequireStaticRuntime -PortableZip $portableZip -Installer $installer

    $sourceCommit = (& git rev-parse HEAD).Trim()
    $dirty = [bool](& git status --porcelain)
    $manifest = [pscustomobject]@{
        source_commit = $sourceCommit
        source_dirty = $dirty
        exe = ($validation | ConvertFrom-Json)
        bundled_license_files = @($licenseFiles | ForEach-Object { 'licenses/' + $_.Destination })
        portable_zip_sha256 = (Get-FileHash -LiteralPath $portableZip -Algorithm SHA256).Hash
        installer_sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
        native_gui_accepted = $false
    }
    $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $delivery 'manifest.json')
    Write-Output ('Reviewable package directory: ' + $delivery)
}
finally {
    Pop-Location
    $env:STATIC_VCRUNTIME = $oldStaticRuntime
    $env:CARGO_ENCODED_RUSTFLAGS = $oldEncodedRustFlags
    $env:RUSTFLAGS = $oldRustFlags
}

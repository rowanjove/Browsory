param([Parameter(Mandatory=$true)][string]$Executable,[switch]$RequireStaticRuntime,[string]$PortableZip,[string]$Installer)
$ErrorActionPreference = 'Stop'
foreach($module in @('Microsoft.PowerShell.Utility','Microsoft.PowerShell.Management')){
    Import-Module (Join-Path $PSHOME ("Modules/$module/$module.psd1")) -Force -ErrorAction Stop
}
$file = Get-Item -LiteralPath $Executable
if ($file.Length -lt 256 -or $file.Length -gt 512MB) { throw 'Unexpected executable size' }
$workspace = Split-Path -Parent $PSScriptRoot
$tauriConfig = Get-Content -LiteralPath (Join-Path $workspace 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if ($tauriConfig.bundle.license -ne 'Apache-2.0' -or $tauriConfig.bundle.licenseFile -ne '../LICENSE') {
    throw 'Installer must declare the Apache-2.0 license and bundle LICENSE'
}
$licenseDestinations = @(
    'licenses/NOTICE',
    'licenses/THIRD_PARTY_NOTICES.md',
    'licenses/OpenSSL-LICENSE.txt',
    'licenses/SQLCipher-LICENSE',
    'licenses/LICENSE-sqlcipher',
    'licenses/LICENSE-libtomcrypt',
    'licenses/webpki-roots-LICENSE.txt'
)
$configuredDestinations = @($tauriConfig.bundle.resources.PSObject.Properties | ForEach-Object { [string]$_.Value })
foreach ($destination in $licenseDestinations) {
    if ($destination -notin $configuredDestinations) { throw "Installer resource is missing $destination" }
}
$licenseSources = @(
    '../LICENSE',
    '../NOTICE',
    '../docs/THIRD_PARTY_NOTICES.md',
    'openssl/LICENSE.txt',
    '../docs/licenses/sqlcipher-src/LICENSE',
    '../docs/licenses/sqlcipher-src/LICENSE-sqlcipher',
    '../docs/licenses/sqlcipher-src/LICENSE-libtomcrypt',
    '../docs/licenses/webpki-roots/LICENSE'
)
foreach ($source in $licenseSources) {
    $sourcePath = [IO.Path]::GetFullPath((Join-Path (Join-Path $workspace 'src-tauri') $source))
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) { throw "Missing license source: $source" }
}
if ($Installer) {
    if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) { throw 'NSIS installer is missing' }
    $licenseFile = Join-Path $workspace 'src-tauri/target/release/nsis/x64/license_file'
    $projectLicense = Join-Path $workspace 'LICENSE'
    if (-not (Test-Path -LiteralPath $licenseFile -PathType Leaf)) { throw 'NSIS license page file is missing' }
    $installerLicenseBytes = [IO.File]::ReadAllBytes($licenseFile)
    $projectLicenseBytes = [IO.File]::ReadAllBytes($projectLicense)
    if ($installerLicenseBytes.Length -ne $projectLicenseBytes.Length + 3 -or
        $installerLicenseBytes[0] -ne 0xEF -or $installerLicenseBytes[1] -ne 0xBB -or $installerLicenseBytes[2] -ne 0xBF) {
        throw 'NSIS license page does not match the project Apache-2.0 license'
    }
    for ($i = 0; $i -lt $projectLicenseBytes.Length; $i++) {
        if ($installerLicenseBytes[$i + 3] -ne $projectLicenseBytes[$i]) { throw 'NSIS license page content differs from LICENSE' }
    }
    $nsisScript = Join-Path $workspace 'src-tauri/target/release/nsis/x64/installer.nsi'
    if (-not (Test-Path -LiteralPath $nsisScript -PathType Leaf)) { throw 'Generated NSIS installer script is missing' }
    $nsisText = Get-Content -LiteralPath $nsisScript -Raw
    foreach ($destination in $licenseDestinations) {
        $oname = '/oname=' + $destination.Replace('/', '\')
        if ($nsisText.IndexOf($oname, [StringComparison]::OrdinalIgnoreCase) -lt 0) {
            throw "NSIS installer script does not include $destination"
        }
    }
}
if ($PortableZip) {
    if (-not (Test-Path -LiteralPath $PortableZip -PathType Leaf)) { throw 'Portable ZIP is missing' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead((Resolve-Path -LiteralPath $PortableZip).Path)
    try {
        $entryNames = @($archive.Entries | ForEach-Object { $_.FullName.Replace('\','/') })
        foreach ($destination in @('licenses/LICENSE') + $licenseDestinations) {
            if ($destination -notin $entryNames) { throw "Portable ZIP is missing $destination" }
        }
    }
    finally { $archive.Dispose() }
}
$bytes = [IO.File]::ReadAllBytes($file.FullName)
function U16([int]$offset) { [BitConverter]::ToUInt16($bytes,$offset) }
function U32([int]$offset) { [BitConverter]::ToUInt32($bytes,$offset) }
if ((U16 0) -ne 0x5A4D) { throw 'Missing DOS signature' }
$pe = [int](U32 0x3c)
if ($pe -lt 64 -or $pe+256 -ge $bytes.Length -or (U32 $pe) -ne 0x4550) { throw 'Invalid PE header' }
if ((U16 ($pe+4)) -ne 0x8664) { throw 'Package is not Windows x64' }
$optional=$pe+24
if ((U16 $optional) -ne 0x20b) { throw 'Expected PE32+' }
$subsystem=U16 ($optional+68)
if($subsystem -ne 2){throw 'Expected Windows GUI subsystem'}
$dllCharacteristics=U16 ($optional+70)
if (($dllCharacteristics -band 0x100) -eq 0 -or ($dllCharacteristics -band 0x40) -eq 0) { throw 'DEP/ASLR flags missing' }
$sections=@()
$table=$optional+(U16 ($pe+20))
for($i=0;$i -lt (U16 ($pe+6));$i++){
    $entry=$table+40*$i
    $sections+=@{rva=[int](U32 ($entry+12));size=[Math]::Max([int](U32 ($entry+8)),[int](U32 ($entry+16)));raw=[int](U32 ($entry+20))}
}
function FileOffset([int]$rva){
    foreach($section in $sections){
        if($rva -ge $section.rva -and $rva -lt $section.rva+$section.size){
            $offset=$rva-$section.rva+$section.raw
            if($offset -lt 0 -or $offset -ge $bytes.Length){throw 'PE RVA outside file'}
            return $offset
        }
    }
    throw 'Unmapped PE RVA'
}
$imports=[Collections.Generic.List[string]]::new()
$importRva=[int](U32 ($optional+120))
if($importRva -ne 0){
    $cursor=FileOffset $importRva
    for($i=0;$i -lt 1024;$i++){
        $nameRva=[int](U32 ($cursor+12));if($nameRva -eq 0){break}
        $start=FileOffset $nameRva;$end=$start
        while($end -lt $bytes.Length -and $bytes[$end] -ne 0 -and $end-$start -lt 512){$end++}
        if($end-$start -ge 512){throw 'Invalid DLL name'}
        $imports.Add([Text.Encoding]::ASCII.GetString($bytes,$start,$end-$start));$cursor+=20
    }
}
if(@($imports|Where-Object{$_ -match '(libcrypto|libssl|sqlcipher|sqlite3).*\.dll$'}).Count -ne 0){throw 'Unexpected external crypto/SQLite DLL dependency'}
# Validate the actual RT_MANIFEST/id=1 resource, rather than searching all bytes.
$resource=FileOffset ([int](U32 ($optional+128)))
function ResourceEntry([int]$directory,[int]$id){
    $count=(U16 ($directory+12))+(U16 ($directory+14))
    for($i=0;$i -lt $count;$i++){
        $entry=$directory+16+8*$i;$name=U32 $entry
        if(($name -band 0x80000000L) -eq 0 -and $name -eq $id){return [long](U32 ($entry+4))}
    }
    throw "PE resource ID $id missing"
}
$type=ResourceEntry $resource 24
if(($type -band 0x80000000L) -eq 0){throw 'Manifest type is not a resource directory'}
$identifier=ResourceEntry ($resource+($type -band 0x7fffffffL)) 1
if(($identifier -band 0x80000000L) -eq 0){throw 'Manifest ID is not a resource directory'}
$language=$resource+($identifier -band 0x7fffffffL)
$data=U32 ($language+20)
if(($data -band 0x80000000L) -ne 0){throw 'Invalid manifest language entry'}
$dataEntry=$resource+$data
$manifestOffset=FileOffset ([int](U32 $dataEntry))
$manifestSize=[int](U32 ($dataEntry+4))
if($manifestSize -lt 32 -or $manifestSize -gt 128KB -or $manifestOffset+$manifestSize -gt $bytes.Length){throw 'Invalid manifest size'}
$manifest=[Text.Encoding]::UTF8.GetString($bytes,$manifestOffset,$manifestSize)
if($manifest -notmatch 'Microsoft\.Windows\.Common-Controls' -or $manifest -notmatch '6\.0\.0\.0'){throw 'Common Controls v6 dependency missing from executable manifest'}
$externalVc=@($imports|Where-Object{$_ -match '^(vcruntime|msvcp|concrt).*\.dll$'}).Count -gt 0
if($RequireStaticRuntime -and $externalVc){throw 'VC runtime must be statically linked for this package; use the Tauri packaging command'}
[pscustomobject]@{
    executable=$file.Name
    file_size_bytes=$file.Length
    sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    architecture='x86_64'
    subsystem=$subsystem
    dep=$true
    aslr=$true
    imports=@($imports)
    external_crypto_sqlite_dlls=$false
    common_controls_v6_manifest=$true
    external_vc_runtime=$externalVc
    scope='Static package validation only; native GUI/runtime acceptance is separate.'
}|ConvertTo-Json -Depth 4

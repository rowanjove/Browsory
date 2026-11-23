param([string]$Executable,[string]$Installer)
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$files = @(
    (Join-Path $workspace 'src-tauri/openssl/lib/libcrypto.lib'),
    (Join-Path $workspace 'src-tauri/openssl/lib/libssl.lib')
)
if ($Executable) { $files += [IO.Path]::GetFullPath($Executable) }
if ($Installer) { $files += [IO.Path]::GetFullPath($Installer) }

$pathPattern = '(?i)(?<![A-Z0-9])[A-Z]:\\[A-Z0-9._ ()-]{1,100}(?:\\[A-Z0-9._ ()-]{1,100})+'
$encodedPathPattern = '(?i)[A-Z]\?3\?2[A-Z][A-Z0-9_]*\?2'
foreach ($path in $files) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Binary path check input is missing: $path" }
    $bytes = [IO.File]::ReadAllBytes($path)
    $ascii = [Text.Encoding]::ASCII.GetString($bytes)
    $utf16 = [Text.Encoding]::Unicode.GetString($bytes)
    $matches = [regex]::Matches($ascii, $pathPattern).Count + [regex]::Matches($utf16, $pathPattern).Count
    $matches += [regex]::Matches($ascii, $encodedPathPattern).Count + [regex]::Matches($utf16, $encodedPathPattern).Count
    if ($matches -gt 0) { throw "Binary contains $matches absolute Windows or MSVC-encoded path string(s): $([IO.Path]::GetFileName($path))" }
    Write-Output ("No absolute Windows paths found: " + [IO.Path]::GetFileName($path))
}

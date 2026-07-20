[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($env:OS -ne "Windows_NT") {
    throw "Portable packaging is supported only on Windows."
}

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($null -eq $cargoCommand) {
    $cargoFallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (-not (Test-Path -LiteralPath $cargoFallback)) {
        throw "Cargo was not found. Install Rust before building a portable archive."
    }

    $cargoExecutable = $cargoFallback
    $env:PATH = "$(Split-Path -Parent $cargoFallback);$env:PATH"
}
else {
    $cargoExecutable = $cargoCommand.Source
}

$configurationPath = Join-Path $repositoryRoot "src-tauri\tauri.conf.json"
$configuration = Get-Content -LiteralPath $configurationPath -Raw | ConvertFrom-Json
$productName = [string]$configuration.package.productName
$version = [string]$configuration.package.version
$archiveStem = "GBFR-Logs-Awa-Edition_${version}_windows-x64-portable"

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot "target\release\bundle\portable"
}

$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
$stagingDirectory = Join-Path $OutputDirectory $archiveStem
$archivePath = Join-Path $OutputDirectory "$archiveStem.zip"
$checksumPath = "$archivePath.sha256"

Push-Location $repositoryRoot
try {
    if (-not $SkipBuild) {
        & $cargoExecutable build --release --package hook
        if ($LASTEXITCODE -ne 0) {
            throw "The release hook build failed."
        }

        & npm run tauri -- build --bundles none --ci
        if ($LASTEXITCODE -ne 0) {
            throw "The release application build failed."
        }
    }

    $releaseDirectory = Join-Path $repositoryRoot "target\release"
    $executablePath = Join-Path $releaseDirectory "$productName.exe"
    $hookPath = Join-Path $releaseDirectory "hook.dll"
    $requiredPaths = @(
        $executablePath,
        $hookPath,
        (Join-Path $repositoryRoot "src-tauri\assets"),
        (Join-Path $repositoryRoot "src-tauri\lang"),
        (Join-Path $repositoryRoot "LICENSE"),
        (Join-Path $repositoryRoot "portable\README.txt")
    )

    foreach ($requiredPath in $requiredPaths) {
        if (-not (Test-Path -LiteralPath $requiredPath)) {
            throw "Required portable file is missing: $requiredPath"
        }
    }

    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    if (Test-Path -LiteralPath $stagingDirectory) {
        Remove-Item -LiteralPath $stagingDirectory -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null

    Copy-Item -LiteralPath $executablePath -Destination $stagingDirectory
    Copy-Item -LiteralPath $hookPath -Destination $stagingDirectory
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "src-tauri\assets") -Destination $stagingDirectory -Recurse
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "src-tauri\lang") -Destination $stagingDirectory -Recurse
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "LICENSE") -Destination $stagingDirectory
    Copy-Item -LiteralPath (Join-Path $repositoryRoot "portable\README.txt") -Destination $stagingDirectory

    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archiveStream = [System.IO.File]::Open($archivePath, [System.IO.FileMode]::CreateNew)
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $archiveStream,
            [System.IO.Compression.ZipArchiveMode]::Create
        )
        try {
            foreach ($file in Get-ChildItem -LiteralPath $stagingDirectory -Recurse -File) {
                $relativePath = $file.FullName.Substring($stagingDirectory.Length).TrimStart("\")
                $entryName = "$archiveStem/$($relativePath.Replace("\", "/"))"
                [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                    $archive,
                    $file.FullName,
                    $entryName,
                    [System.IO.Compression.CompressionLevel]::Optimal
                ) | Out-Null
            }
        }
        finally {
            $archive.Dispose()
        }
    }
    finally {
        $archiveStream.Dispose()
    }

    $checksum = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    "$checksum  $([System.IO.Path]::GetFileName($archivePath))" | Set-Content -LiteralPath $checksumPath -Encoding ascii

    Write-Host "Portable archive: $archivePath"
    Write-Host "SHA-256 checksum: $checksumPath"
}
finally {
    Pop-Location
}

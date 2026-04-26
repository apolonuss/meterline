param(
    [string]$Version = "latest",
    [string]$InstallRoot = "$env:LOCALAPPDATA\Meterline",
    [switch]$FromSource,
    [switch]$NoPath
)

$ErrorActionPreference = "Stop"

$Repo = "apolonuss/meterline"
$BinaryName = "meterline.exe"
$BinDir = Join-Path $InstallRoot "bin"
$ReleaseBase = "https://github.com/$Repo/releases"

function Write-Step($Message) {
    Write-Host "meterline: $Message"
}

function Get-AssetName {
    $arch = switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { "x86_64" }
        "ARM64" { "aarch64" }
        default { throw "Unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE" }
    }

    "meterline-windows-$arch.zip"
}

function Get-ReleaseUrl($AssetName) {
    if ($Version -eq "latest") {
        return "$ReleaseBase/latest/download/$AssetName"
    }

    $tag = $Version
    if (-not $tag.StartsWith("v")) {
        $tag = "v$tag"
    }
    "$ReleaseBase/download/$tag/$AssetName"
}

function Install-FromRelease {
    $asset = Get-AssetName
    $url = Get-ReleaseUrl $asset
    $temp = Join-Path ([System.IO.Path]::GetTempPath()) "meterline-install-$([guid]::NewGuid())"
    $zipPath = Join-Path $temp $asset
    $extractDir = Join-Path $temp "extract"

    New-Item -ItemType Directory -Force -Path $temp, $extractDir | Out-Null

    try {
        Write-Step "downloading $asset"
        Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing
        Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

        $binary = Get-ChildItem -Path $extractDir -Filter $BinaryName -Recurse | Select-Object -First 1
        if (-not $binary) {
            throw "Release archive did not contain $BinaryName"
        }

        New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
        Copy-Item -Path $binary.FullName -Destination (Join-Path $BinDir $BinaryName) -Force
        return $true
    }
    catch {
        Write-Step "release install unavailable: $($_.Exception.Message)"
        return $false
    }
    finally {
        Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Install-FromSource {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if (-not $cargo) {
        throw @"
No Meterline release asset was available, and Rust/Cargo was not found.

Install Rust from https://rustup.rs, then rerun this installer, or download a
prebuilt Meterline release once one is published.
"@
    }

    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "meterline-cargo-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

    try {
        Write-Step "building from source with cargo"
        cargo install --git "https://github.com/$Repo" --locked --root $tempRoot
        New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
        Copy-Item -Path (Join-Path $tempRoot "bin\$BinaryName") -Destination (Join-Path $BinDir $BinaryName) -Force
    }
    finally {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Add-ToUserPath {
    if ($NoPath) {
        return
    }

    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @()
    if ($current) {
        $parts = $current -split ";" | Where-Object { $_ }
    }

    if ($parts -notcontains $BinDir) {
        $next = (@($parts) + $BinDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $next, "User")
        Write-Step "added $BinDir to your user PATH"
        Write-Step "open a new terminal before running meterline"
    }
}

if (-not $FromSource) {
    $installed = Install-FromRelease
}
else {
    $installed = $false
}

if (-not $installed) {
    Install-FromSource
}

Add-ToUserPath
Write-Step "installed to $(Join-Path $BinDir $BinaryName)"
Write-Step "try: meterline init"

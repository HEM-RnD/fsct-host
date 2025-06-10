#
# FSCT Driver Service Windows Installer Build Script
# 
# This script builds the FSCT Driver Service and creates Windows installers (MSI and EXE bundle).
# It handles:
#   1. Building the Rust service
#   2. Signing the executable (if enabled)
#   3. Downloading the Visual C++ Redistributable
#   4. Creating an MSI installer using WiX Toolset v6.0
#   5. Creating an EXE bundle installer
#   6. Signing the installers (if enabled)
#
# Requirements:
#   - Rust (cargo)
#       - cargo about
#   - WiX Toolset v6.0
#   - signtool (if signing is enabled)
#   - pandoc
#
param(
    [Parameter()][int]$BuildNumber = 0,
    [switch]$NoSign,
    [switch]$NoDwnld,
    [switch]$Help
)

# Store initial location
$initialLocation = Get-Location
$scriptLocation = Split-Path -Parent $MyInvocation.MyCommand.Path
$projectLocation = Split-Path -Parent $scriptLocation

# Ensure we return to initial location on exit
try
{
    # Display help information if requested
    if ($Help)
    {
        Write-Host "FSCT Driver Service Windows Installer Build Script"
        Write-Host "Usage: .\build_windows_installer.ps1 [-NoSign] [-SkipDwnld] [-Help]"
        Write-Host ""
        Write-Host "Options:"
        Write-Host "  -NoSign    Skip signing of executables and installers"
        Write-Host "  -NoDwnld   Skip downloading Visual C++ Redistributable and wix extensions"
        Write-Host "  -Help      Display this help message"
        exit 0
    }
    
    # === Configuration ===
    $PROJECT_NAME = "fsct_driver_service"
    $VCREDIST_URL = "https://aka.ms/vs/17/release/vc_redist.x64.exe"
    $VCREDIST_EXE = "vc_redist.x64.exe"
    $SIGN_CERT = "cert.pfx"
    $SIGN_PASSWORD = "password"
    $SIGN_ENABLED = $true
    $DOWNLOAD_ENABLE = $true

    $PROJECT_DIR = $projectLocation
    $WIX_SOURCE_DIR = Join-Path $projectLocation "ports\native\packages\windows"
    $BUILD_DIR = Join-Path $projectLocation "target\wix_build"
    $EULA_DIR = Join-Path $projectLocation "ports\native"
    $EULA_RTF = Join-Path $BUILD_DIR "EULA.rtf"

    # Parse command line arguments
    if ($NoSign)
    {
        $SIGN_ENABLED = $false
    }
    if ($NoDwnld)
    {
        $DOWNLOAD_ENABLE = $false
    }

    # === Checking dependencies ===
    function Check-Tool
    {
        param(
            [string]$toolName
        )

        if (-not (Get-Command $toolName -ErrorAction SilentlyContinue))
        {
            Write-Error "[ERROR] Required tool '$toolName' is not installed or not in PATH."
            exit 1
        }
    }

    # Check for required tools
    Check-Tool -toolName "cargo"
    Check-Tool -toolName "wix"

    # Check if wix is available in PATH
    $wixVersion = & wix --version 2>&1
    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] WiX Toolset v6.0 not found in PATH."
        Write-Error "[ERROR] Please ensure WiX Toolset v6.0 is installed correctly and added to PATH."
        exit 1
    }

    Write-Host "[INFO] Using WiX Toolset: $wixVersion"

    # Check if signtool is available in PATH
    if ($SIGN_ENABLED)
    {
        Check-Tool -toolName "signtool"
    }
    else
    {
        Write-Host "[INFO] Skipping signtool check in no-sign mode"
    }

    # Check if pandoc is available in PATH
    Check-Tool -toolName "pandoc"
    $pandocVersion = (& pandoc --version 2>&1)[0].Split(" ")[-1]
    Write-Host "[INFO] Using pandoc: $pandocVersion"

    # Check if cargo about is installed
    try
    {
        $cargoAboutVersion = (& cargo about -V 2>&1).Split(" ")[-1]
        Write-Host "[INFO] Using cargo about: $cargoAboutVersion"
    }
    catch {
        Write-Error "[ERROR] Required tool 'cargo about' is not installed."
        exit 1
    }


    # === Prepare build directory ===
    Write-Host "[INFO] Preparing build directory..."
    if ((Test-Path $BUILD_DIR) -and (-not $DOWNLOAD_ENABLE))
    {
        Write-Host "[INFO] Skipping build directory cleanup (download disabled)"
    }
    elseif (Test-Path $BUILD_DIR)
    {
        try
        {
            Remove-Item -Path $BUILD_DIR -Recurse -Force
        }
        catch
        {
            Write-Error "[ERROR] Failed to remove existing build directory"
            exit 1
        }
    }

    try
    {
        New-Item -Path $BUILD_DIR -ItemType Directory -Force | Out-Null
    }
    catch
    {
        Write-Error "[ERROR] Failed to create build directory"
        exit 1
    }

    # === Building service ===
    Write-Host "[INFO] Building Rust service..."
    try
    {
        cargo build --release
        Copy-Item "target\release\$PROJECT_NAME.exe" "$BUILD_DIR\$PROJECT_NAME.exe" -Force
    }
    catch
    {
        Write-Error "[ERROR] Failed to build Rust service"
        exit 1
    }


    # === Get package version ===
    Write-Host "[INFO] Getting package version..."
    function Get-CargoVersion
    {
        $cargoMetadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
        $package = $cargoMetadata.packages | Where-Object { $_.name -eq $PROJECT_NAME }
        return $package.version
    }

    $packageVersion = Get-CargoVersion

    if ( [string]::IsNullOrEmpty($packageVersion))
    {
        Write-Error "[ERROR] Failed to get package version"
        exit 1
    }

    Write-Host "[INFO] Package version: $packageVersion"

    $installerVersion = "$packageVersion.$BuildNumber"

    Write-Host "[INFO] Installer version: $installerVersion"

    # === Signing EXE ===
    if ($SIGN_ENABLED)
    {
        Write-Host "[INFO] Signing EXE..."
        $signResult = signtool sign /f $SIGN_CERT /p $SIGN_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 "$BUILD_DIR\$PROJECT_NAME.exe" 2>&1
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to sign EXE"
            Write-Error "[ERROR] Error details: $signResult"
            exit 1
        }
        Write-Host "[INFO] EXE signed successfully"
    }
    else
    {
        Write-Host "[INFO] Skipping EXE signing (developer mode)"
    }

    # === Downloading VC Redist ===
    if ((-not (Test-Path "$BUILD_DIR\$VCREDIST_EXE")) -and $DOWNLOAD_ENABLE)
    {
        Write-Host "[INFO] Downloading Visual C++ Redistributable..."
        try
        {
            Invoke-WebRequest -Uri $VCREDIST_URL -OutFile "$BUILD_DIR\$VCREDIST_EXE"
        }
        catch
        {
            Write-Error "[ERROR] Failed to download Visual C++ Redistributable"
            exit 1
        }
    }
    elseif (-not $DOWNLOAD_ENABLE)
    {
        Write-Host "[INFO] Skipping Visual C++ Redistributable download (download disabled)"
    }

    # === Copying WiX source files ===
    Write-Host "[INFO] Copying WiX source files..."
    try
    {
        Copy-Item "$WIX_SOURCE_DIR\fsct_service_installer.wxs" "$BUILD_DIR\fsct_service_installer.wxs" -Force
        Copy-Item "$WIX_SOURCE_DIR\fsct_installer_bundle.wxs" "$BUILD_DIR\fsct_installer_bundle.wxs" -Force
        Copy-Item "$PROJECT_DIR\LICENSE-FSCT.md" "$BUILD_DIR\LICENSE-FSCT.md" -Force
        Copy-Item "$PROJECT_DIR\NOTICE" "$BUILD_DIR\NOTICE" -Force
    }
    catch
    {
        Write-Error "[ERROR] Failed to copy WiX source files"
        exit 1
    }
    
    # === Generating EULA ===
    Write-Host "[INFO] Generating RTF EULA..."
    try
    {
        # Convert to RTF using pandoc
        $pandocResult = & pandoc --from markdown --to rtf -s -o $EULA_RTF "$EULA_DIR\FSCT_Driver_EULA.md" 2>&1

        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to generate RTF license"
            Write-Error "[ERROR] Pandoc error: $pandocResult"
            throw
        }
    }
    catch
    {
        Write-Error "[ERROR] Failed to generate RTF license: $_"
        throw
    }

    # === Generating Licenses ===
    & cargo about generate -c about.toml -m ports/native/Cargo.toml licenses.hbs    `
        -o "$BUILD_DIR/LICENSES.md" 2>&1

    Set-Location $BUILD_DIR
    # === Installing WiX extensions ===

    Write-Host "[INFO] Installing WixToolset.Util.wixext extension..."
    $utilResult = & wix extension add WixToolset.Util.wixext 2>&1
    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] Failed to install WixToolset.Util.wixext"
        Write-Error "[ERROR] Error details: $utilResult"
        exit 1
    }

    Write-Host "[INFO] Installing WixToolset.BootstrapperApplications.wixext extension..."
    $utilResult = & wix extension add WixToolset.BootstrapperApplications.wixext 2>&1
    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] Failed to install WixToolset.BootstrapperApplications.wixext"
        Write-Error "[ERROR] Error details: $utilResult"
        exit 1
    }

    Write-Host "[INFO] WiX extensions ready"

    # === WiX Compilation ===
    Write-Host "[INFO] Compiling WiX files..."
    $msiResult = & wix build -arch x64 -d Version=$installerVersion -ext WixToolset.Util.wixext         `
        -o FSCTServiceInstaller.msi fsct_service_installer.wxs 2>&1
    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] WiX compilation failed"
        Write-Error "[ERROR] Error details: $msiResult"
        exit 1
    }

    if (-not (Test-Path "FSCTServiceInstaller.msi"))
    {
        Write-Error "[ERROR] WiX compilation did not produce expected MSI file"
        exit 1
    }

    # === Bundle Compilation ===
    Write-Host "[INFO] Compiling bundle installer..."
    $bundleResult = & wix build -arch x64 -d Version=$installerVersion -d EULA=$EULA_RTF                `
        -ext WixToolset.Util.wixext -ext WixToolset.BootstrapperApplications.wixext                     `
        -o FSCTDriverInstaller.exe fsct_installer_bundle.wxs 2>&1

    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] Bundle compilation failed"
        Write-Error "[ERROR] Error details: $bundleResult"
        exit 1
    }

    if (-not (Test-Path "FSCTDriverInstaller.exe"))
    {
        Write-Error "[ERROR] Bundle compilation did not produce expected EXE file"
        exit 1
    }
    Set-Location $initialLocation

    # === Signing MSI and EXE ===
    if ($SIGN_ENABLED)
    {
        Write-Host "[INFO] Signing MSI..."
        $msiSignResult = signtool sign /f $SIGN_CERT /p $SIGN_PASSWORD /fd SHA256 `
            /tr http://timestamp.digicert.com /td SHA256 "$BUILD_DIR\FSCTServiceInstaller.msi" 2>&1
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to sign MSI"
            Write-Error "[ERROR] Error details: $msiSignResult"
            exit 1
        }
        Write-Host "[INFO] MSI signed successfully"

        Write-Host "[INFO] Signing bundle EXE..."
        $bundleSignResult = signtool sign /f $SIGN_CERT /p $SIGN_PASSWORD /fd SHA256 `
            /tr http://timestamp.digicert.com /td SHA256 "$BUILD_DIR\FSCTDriverInstaller.exe" 2>&1
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to sign bundle EXE"
            Write-Error "[ERROR] Error details: $bundleSignResult"
            exit 1
        }
        Write-Host "[INFO] Bundle EXE signed successfully"
    }

    # === Done ===
    Write-Host "[SUCCESS] Installers generated:"
    Write-Host "  - $BUILD_DIR\FSCTServiceInstaller.msi"
    Write-Host "  - $BUILD_DIR\FSCTDriverInstaller.exe"

}
finally {
    Set-Location $initialLocation
}

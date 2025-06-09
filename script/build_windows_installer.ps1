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
#   - WiX Toolset v6.0
#   - curl
#   - signtool (if signing is enabled)
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

    $WIX_SOURCE_DIR = Join-Path $projectLocation "ports\native\packages\windows"
    $BUILD_DIR = Join-Path $projectLocation "target\wix_build"

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
    if ($DOWNLOAD_ENABLE)
    {
        Check-Tool -toolName "curl"
    }
    else
    {
        Write-Host "[INFO] Skipping curl check in no-download mode"
    }

    # Check if wix is available in PATH
    $wixVersion = & wix --version 2>&1
    if ($LASTEXITCODE -ne 0)
    {
        Write-Error "[ERROR] WiX Toolset v6.0 not found in PATH."
        Write-Error "[ERROR] Please ensure WiX Toolset v6.0 is installed correctly and added to PATH."
        exit 1
    }

    Write-Host "[INFO] Using WiX Toolset: $wixVersion"

    if ($SIGN_ENABLED)
    {
        Check-Tool -toolName "signtool"
    }
    else
    {
        Write-Host "[INFO] Skipping signtool check in no-sign mode"
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
        New-Item -Path "$BUILD_DIR\obj" -ItemType Directory -Force | Out-Null
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
        Copy-Item "$WIX_SOURCE_DIR\FSCT_Driver_EULA.rtf" "$BUILD_DIR\FSCT_Driver_EULA.rtf" -Force
    }
    catch
    {
        Write-Error "[ERROR] Failed to copy WiX source files"
        exit 1
    }

    # === Skip WiX fragment generation ===
    Write-Host "[INFO] Skipping WiX fragment generation..."
    Set-Location $BUILD_DIR

    # Find extension DLLs
    Write-Host "[INFO] Finding WiX extension DLLs..."
    $balDllPath = Join-Path $initialLocation ".wix\extensions\WixToolset.Bal.wixext\6.0.1\wixext6\WixToolset.BootstrapperApplications.wixext.dll"

    if (-not (Test-Path $balDllPath))
    {
        Write-Error "[ERROR] WixToolset.Bal.wixext DLL not found at: $balDllPath"
        exit 1
    }

    # Check if WixToolset.Util.wixext is installed
    $extensionList = & wix extension list 2>&1
    if ($extensionList -notmatch "WixToolset.Util.wixext")
    {
        Write-Host "[INFO] Installing WixToolset.Util.wixext extension..."
        $utilResult = & wix extension add WixToolset.Util.wixext 2>&1
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to install WixToolset.Util.wixext"
            Write-Error "[ERROR] Error details: $utilResult"
            exit 1
        }
    }

    Write-Host "[INFO] Found Bal extension DLL: $balDllPath"
    Write-Host "[INFO] WiX extensions ready"

    # === WiX Compilation ===
    Write-Host "[INFO] Compiling WiX files..."
    $msiResult = & wix build -arch x64 -d Version=$installerVersion -ext WixToolset.Util.wixext -o FSCTServiceInstaller.msi fsct_service_installer.wxs 2>&1
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
    $bundleResult = & wix build -arch x64 -d Version=$installerVersion -ext WixToolset.Util.wixext -ext $balDllPath -o FSCTDriverInstaller.exe fsct_installer_bundle.wxs 2>&1
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
        $msiSignResult = signtool sign /f $SIGN_CERT /p $SIGN_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 "$BUILD_DIR\FSCTServiceInstaller.msi" 2>&1
        if ($LASTEXITCODE -ne 0)
        {
            Write-Error "[ERROR] Failed to sign MSI"
            Write-Error "[ERROR] Error details: $msiSignResult"
            exit 1
        }
        Write-Host "[INFO] MSI signed successfully"

        Write-Host "[INFO] Signing bundle EXE..."
        $bundleSignResult = signtool sign /f $SIGN_CERT /p $SIGN_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 "$BUILD_DIR\FSCTDriverInstaller.exe" 2>&1
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

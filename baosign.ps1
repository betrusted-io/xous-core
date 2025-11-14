# Image Signing and Deployment Automation Script

# Invoke with e.g. .\baosign.ps1 -Config dabao
# From the root of xous-core. Must be run in a powershell session with administrator privileges
# in order to access the signing token.
#
# TODO: port to a Linux environment so signing can be done on CI-generated builds - once we
# have setup the full signing process.

param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("baremetal", "dabao", "baosec", "bootloader", "kernel", "loader", "swap", "apps", "boot1", "alt-boot1")]
    [string]$Config,

    [Parameter(Mandatory = $false)]
    [string]$CredentialFile = "beta.json",

    [Parameter(Mandatory = $false)]
    [string]$Target = "bunnie@10.0.245.183:code/jtag-tools/"
)

# Configuration
$TARGET = $Target
$SIGNING_DIR = ".\signing\fido-signer"

# Define configurations and their associated images with function codes
# Modify these mappings according to your actual image names and function codes
$configurations = @{
    "bootloader" = @(
        @{ Image = "bao1x-boot0.img"; FunctionCode = "boot0" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "bao1x-boot1.img"; FunctionCode = "boot1" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "bao1x-alt-boot1.img"; FunctionCode = "loader" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" }
    )
    "baremetal"  = @(
        @{ Image = "baremetal.img"; FunctionCode = "baremetal" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" }
    )
    "dabao"      = @(
        @{ Image = "bao1x-boot0.img"; FunctionCode = "boot0" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "bao1x-boot1.img"; FunctionCode = "boot1" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "loader.bin"; FunctionCode = "loader" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
        @{ Image = "xous.img"; FunctionCode = "kernel" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
        @{ Image = "apps.img"; FunctionCode = "app" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "baosec"     = @(
        @{ Image = "bao1x-boot0.img"; FunctionCode = "boot0" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "bao1x-boot1.img"; FunctionCode = "boot1" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" },
        @{ Image = "loader.bin"; FunctionCode = "loader" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
        @{ Image = "xous.img"; FunctionCode = "kernel" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
        @{ Image = "swap.img"; FunctionCode = "swap" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "kernel"     = @(
        @{ Image = "xous.img"; FunctionCode = "kernel" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "loader"     = @(
        @{ Image = "loader.bin"; FunctionCode = "loader" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "swap"       = @(
        @{ Image = "swap.img"; FunctionCode = "swap" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "apps"       = @(
        @{ Image = "apps.img"; FunctionCode = "app" ; TargetDir = "target\riscv32imac-unknown-xous-elf\release" }
    )
    "boot1"      = @(
        @{ Image = "bao1x-boot1.img"; FunctionCode = "boot1" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" }
    )
    "alt-boot1"  = @(
        @{ Image = "bao1x-alt-boot1.img"; FunctionCode = "baremetal" ; TargetDir = "target\riscv32imac-unknown-none-elf\release" }
    )
}

# Color output functions for better visibility
function Write-Status {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host $Message -ForegroundColor Green
}

function Write-Error {
    param([string]$Message)
    Write-Host "ERROR: $Message" -ForegroundColor Red
}

function Write-Warning {
    param([string]$Message)
    Write-Host "WARNING: $Message" -ForegroundColor Yellow
}

# Function to calculate MD5 hash
function Get-MD5Hash {
    param([string]$FilePath)

    if (Test-Path $FilePath) {
        $hash = Get-FileHash -Path $FilePath -Algorithm MD5
        return $hash.Hash.ToLower()
    }
    else {
        Write-Warning "File not found for MD5 calculation: $FilePath"
        return $null
    }
}

# Function to copy files via SSH
function Copy-ToSSH {
    param(
        [string]$LocalPath,
        [string]$RemoteTarget
    )

    if (Test-Path $LocalPath) {
        try {
            # Using scp for file transfer
            $scpCommand = "scp `"$LocalPath`" `"$RemoteTarget`""
            Write-Status "  Copying: $LocalPath -> $RemoteTarget"

            $result = Invoke-Expression $scpCommand 2>&1
            if ($LASTEXITCODE -eq 0) {
                Write-Success "  Successfully copied: $(Split-Path $LocalPath -Leaf)"
                return $true
            }
            else {
                Write-Error "Failed to copy file: $result"
                return $false
            }
        }
        catch {
            Write-Error "Exception during SCP: $_"
            return $false
        }
    }
    else {
        Write-Error "Local file not found: $LocalPath"
        return $false
    }
}

# Main processing loop
Write-Host "`n==========================================" -ForegroundColor Blue
Write-Host "   Image Signing and Deployment Script    " -ForegroundColor Blue
Write-Host "==========================================" -ForegroundColor Blue
Write-Host ""
Write-Status "Configuration: $Config"
Write-Status "Target SSH: $TARGET"
Write-Status "Credential File: $CredentialFile"
Write-Host ""

$totalImages = 0
$successfulImages = 0
$failedImages = 0

Write-Host "`n------------------------------------------" -ForegroundColor Yellow
Write-Host "  Processing Configuration: $Config" -ForegroundColor Yellow
Write-Host "------------------------------------------" -ForegroundColor Yellow

# Process only the selected configuration
$selectedConfig = $configurations[$Config]

foreach ($imageSpec in $selectedConfig) {
    $totalImages++
    $imageName = $imageSpec.Image
    $functionCode = $imageSpec.FunctionCode
    $target_dir = $imageSpec.TargetDir
    $imagePath = Join-Path $target_dir $imageName
    $uf2Name = $imageName -replace '\.(?:bin|img)$', '.uf2'
    $uf2Path = Join-Path $target_dir $uf2Name

    Write-Host "`nProcessing: $imageName (Function: $functionCode)" -ForegroundColor White

    # Check if image exists
    if (-not (Test-Path $imagePath)) {
        Write-Warning "Image not found: $imagePath (skipping)"
        $failedImages++
        continue
    }

    # Change to signing directory
    Push-Location $SIGNING_DIR

    try {
        # Build the cargo run command
        $cargoCmd = @(
            "cargo", "run", "--",
            "--credential-file", "..\credentials\$CredentialFile",
            "--file", "..\..\$imagePath",
            "--function-code", $functionCode
        )

        Write-Status "Signing image..."
        Write-Status "Command: $($cargoCmd -join ' ')"

        # Execute the signing command
        & cargo run -- --credential-file "..\credentials\$CredentialFile" --file "..\..\$imagePath" --function-code $functionCode 2>&1 | Tee-Object -Variable result
        # $result = cargo run -- --credential-file "..\credentials\$CredentialFile" --file "..\..\$imagePath" --function-code $functionCode | Tee-Object -Variable result

        if ($LASTEXITCODE -eq 0) {
            Write-Success "Successfully signed: $imageName"

            # Return to root directory
            Pop-Location

            # Calculate and display MD5 checksums
            Write-Status "Calculating MD5 checksums..."

            $imgMD5 = Get-MD5Hash -FilePath $imagePath
            if ($imgMD5) {
                Write-Host "  MD5 ($imageName): $imgMD5" -ForegroundColor Gray
            }

            $uf2MD5 = Get-MD5Hash -FilePath $uf2Path
            if ($uf2MD5) {
                Write-Host "  MD5 ($uf2Name): $uf2MD5" -ForegroundColor Gray
            }

            # Copy files to SSH target
            Write-Status "Copying files to remote host..."

            $imgCopySuccess = Copy-ToSSH -LocalPath $imagePath -RemoteTarget $TARGET
            $uf2CopySuccess = Copy-ToSSH -LocalPath $uf2Path -RemoteTarget $TARGET

            if ($imgCopySuccess -and $uf2CopySuccess) {
                Write-Success "Completed processing: $imageName"
                $successfulImages++
            }
            else {
                Write-Warning "Partial completion for: $imageName (signing OK, copy failed)"
                $failedImages++
            }

        }
        else {
            Write-Error "Failed to sign image: $imageName"
            Write-Error "Error output: $result"
            Pop-Location
            $failedImages++
        }

    }
    catch {
        Write-Error "Exception during signing: $_"
        Pop-Location
        $failedImages++
    }
}

# Summary report
Write-Host "`n==========================================" -ForegroundColor Blue
Write-Host "              Summary Report              " -ForegroundColor Blue
Write-Host "==========================================" -ForegroundColor Blue
Write-Host "Total images processed: $totalImages" -ForegroundColor White
Write-Success "Successfully processed: $successfulImages"
if ($failedImages -gt 0) {
    Write-Error "Failed: $failedImages"
}
else {
    Write-Host "Failed: 0" -ForegroundColor White
}

# Exit with appropriate code
if ($failedImages -gt 0) {
    exit 1
}
else {
    Write-Host "`nAll operations completed successfully!" -ForegroundColor Green
    exit 0
}
#
# Prepares bundled resources for distribution on Windows.
#
# This script copies resources that should be bundled with Torch into a
# destination directory. It is used by the Windows build script.
#
# Usage:
#   prepare_bundled_resources.ps1 <destination_directory>
#
# Arguments:
#   destination_directory: The directory where resources should be installed.

Param(
    [Parameter(Mandatory = $true)]
    [String]$DestinationDir
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Path $MyInvocation.MyCommand.Path -Parent
$RepoRoot = (Get-Item "$ScriptDir\..\.." | Select-Object -ExpandProperty FullName)
$ResourcesSource = Join-Path $RepoRoot 'resources'

# Validate that the source resources directory exists
if (-Not (Test-Path $ResourcesSource -PathType Container)) {
    Write-Error "Resources directory not found at $ResourcesSource"
    exit 1
}

# Create the destination directory if it doesn't exist
if (-Not (Test-Path $DestinationDir)) {
    New-Item -ItemType Directory -Path $DestinationDir -Force | Out-Null
}

# Torch's own licence texts and notice, required alongside the distributed binary.
foreach ($File in 'LICENSE-AGPL', 'LICENSE-MIT', 'NOTICE') {
    $Source = Join-Path $RepoRoot $File
    if (-Not (Test-Path $Source -PathType Leaf)) {
        Write-Error "$File not found at $Source"
        exit 1
    }
    Copy-Item -Path $Source -Destination (Join-Path $DestinationDir $File) -Force
}

# Generate third-party license attribution.
#
# Additional (non-Cargo) third-party license files to include in the output.
# When adding a new third-party component to the bundle, add its license file
# to the repo alongside the component and add an entry here.
# Cross-platform components:
$AdditionalLicenses = @(
    @{ Name = 'Alacritty (alacritty_terminal)'; License = 'Apache-2.0'; Path = 'crates\torch_terminal\src\model\LICENSE-ALACRITTY' },
    @{ Name = 'Hack Font'; License = 'MIT'; Path = 'app\assets\bundled\fonts\hack\LICENSE.md' },
    @{ Name = 'Roboto Font'; License = 'SIL Open Font License'; Path = 'app\assets\bundled\fonts\roboto\LICENSE.txt' },
    @{ Name = 'bash-preexec'; License = 'MIT'; Path = 'app\assets\bundled\bootstrap\bash-preexec-LICENSE.md' },
    @{ Name = 'Windows Terminal'; License = 'MIT'; Path = 'app\assets\windows\LICENSE-WINDOWS-TERMINAL' },
    @{ Name = 'GitHub Desktop'; License = 'MIT'; Path = 'app\src\code_review\GITHUB-DESKTOP-LICENSE' },
    @{ Name = 'command-corrections (and thefuck)'; License = 'MIT'; Path = 'script\bundled_licenses\LICENSE-COMMAND-CORRECTIONS' },
    @{ Name = 'warp-command-signatures, warp-completion-metadata'; License = 'MIT'; Path = 'script\bundled_licenses\LICENSE-COMMAND-SIGNATURES' },
    @{ Name = 'warp-workflows, warp-workflows-types'; License = 'Apache-2.0'; Path = 'script\bundled_licenses\LICENSE-WORKFLOWS' }
)
# Windows-only components:
$AdditionalLicenses += @(
    @{ Name = 'OpenConsole / ConPTY (Windows Terminal)'; License = 'MIT'; Path = 'app\assets\windows\LICENSE-WINDOWS-TERMINAL' },
    @{ Name = 'DirectX Shader Compiler'; License = 'NCSA'; Path = 'app\assets\windows\LICENSE-DXC' }
)

$LicensesOutput = Join-Path $DestinationDir 'THIRD_PARTY_LICENSES.txt'
Write-Output "Generating third-party licenses at $LicensesOutput"
cargo about generate --workspace --manifest-path "$RepoRoot\Cargo.toml" -c "$RepoRoot\about.toml" -o "$LicensesOutput" "$RepoRoot\about.hbs"
if (-Not $?) {
    Write-Error 'Failed to generate third-party licenses'
    exit 1
}

# Append additional (non-Cargo) third-party licenses.
foreach ($entry in $AdditionalLicenses) {
    $LicenseFile = Join-Path $RepoRoot $entry.Path
    if (-Not (Test-Path $LicenseFile)) {
        Write-Error "License file not found: $LicenseFile"
        exit 1
    }
    Add-Content -Path $LicensesOutput -Value ''
    Add-Content -Path $LicensesOutput -Value "$($entry.Name) ($($entry.License))"
    Add-Content -Path $LicensesOutput -Value ('-' * 80)
    Get-Content -Path $LicenseFile | Add-Content -Path $LicensesOutput
    Add-Content -Path $LicensesOutput -Value ''
}

# Generate settings JSON schema unless explicitly skipped.
if ($env:SKIP_SETTINGS_SCHEMA -ne '1') {
    $SchemaOutput = Join-Path $DestinationDir 'settings_schema.json'

    if ($env:SETTINGS_SCHEMA_EXECUTABLE -and $env:SETTINGS_SCHEMA_SOURCE) {
        Write-Error 'SETTINGS_SCHEMA_EXECUTABLE and SETTINGS_SCHEMA_SOURCE are mutually exclusive.'
        exit 1
    } elseif ($env:SETTINGS_SCHEMA_EXECUTABLE) {
        if (-Not (Test-Path $env:SETTINGS_SCHEMA_EXECUTABLE -PathType Leaf)) {
            Write-Error "Settings schema executable does not exist: $env:SETTINGS_SCHEMA_EXECUTABLE"
            exit 1
        }
        Write-Output "Generating settings schema at $SchemaOutput"
        $Process = Start-Process `
            -FilePath $env:SETTINGS_SCHEMA_EXECUTABLE `
            -ArgumentList @('dump-settings-schema', $SchemaOutput) `
            -NoNewWindow `
            -PassThru `
            -Wait
        if ($Process.ExitCode -ne 0) {
            Write-Error "Failed to generate settings schema: executable exited with code $($Process.ExitCode)"
            exit 1
        }
    } elseif ($env:SETTINGS_SCHEMA_SOURCE) {
        if (-Not (Test-Path $env:SETTINGS_SCHEMA_SOURCE -PathType Leaf)) {
            Write-Error "Settings schema source does not exist: $env:SETTINGS_SCHEMA_SOURCE"
            exit 1
        }
        Write-Output "Copying settings schema to $SchemaOutput"
        Copy-Item -Path $env:SETTINGS_SCHEMA_SOURCE -Destination $SchemaOutput -Force
    } else {
        Write-Error 'Set SETTINGS_SCHEMA_EXECUTABLE or SETTINGS_SCHEMA_SOURCE when preparing a settings schema.'
        exit 1
    }

    if (-Not (Test-Path $SchemaOutput -PathType Leaf) -or (Get-Item $SchemaOutput).Length -eq 0) {
        Write-Error "Settings schema was not written to $SchemaOutput"
        exit 1
    }
}

Write-Output "Successfully prepared bundled resources in $DestinationDir"

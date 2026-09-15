[CmdletBinding()]
param(
    [string] $BundleScpSource,
    [string] $InstallationDirectory,
    [string] $ConfigPath,
    [string] $AgentExtraSocket,
    [string] $ClientCaCert,
    [string] $ListenAddress,
    [Nullable[int]] $TailscaleListenPort,
    [uri] $CaUrl,
    [ValidatePattern('^[A-Fa-f0-9]{64}$')] [string] $CaFingerprint,
    [string] $Provisioner,
    [string] $CommonName,
    [string[]] $DnsName,
    [string] $StepExecutable,
    [ValidateRange(1, 65535)] [int] $MaxConnections,
    [ValidateRange(60, 604800)] [int] $RenewalIntervalSeconds,
    [string] $BridgeServiceName,
    [System.Management.Automation.PSCredential] $Credential,
    [switch] $ReplaceCertificate
)

$ErrorActionPreference = 'Stop'
$script:InstallParameters = $PSBoundParameters

function Assert-Elevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this script from an elevated PowerShell session.'
    }
}

function Read-InstallerConfig([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        return $null
    }
    try {
        return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    } catch {
        throw "Could not parse installer configuration ${Path}: $($_.Exception.Message)"
    }
}

function Resolve-ConfigValue([string] $Name, $Config, $DefaultValue = $null) {
    if ($script:InstallParameters.ContainsKey($Name)) {
        return Get-Variable -Name $Name -Scope Script -ValueOnly
    }
    if ($Config -and $Config.PSObject.Properties.Name -contains $Name) {
        return $Config.$Name
    }
    return $DefaultValue
}

function Require-Value([string] $Name, $Value) {
    if ($null -eq $Value -or ($Value -is [string] -and [string]::IsNullOrWhiteSpace($Value))) {
        throw "Specify -$Name on the command line or set it in the installer configuration."
    }
    return $Value
}

function Assert-File([string] $Description, [string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Description does not exist: $Path"
    }
}

function Wait-ServiceRemoved([string] $Name) {
    $deadline = (Get-Date).AddSeconds(30)
    while (Get-Service -Name $Name -ErrorAction SilentlyContinue) {
        if ((Get-Date) -ge $deadline) {
            throw "Timed out waiting for service $Name to be removed."
        }
        Start-Sleep -Milliseconds 500
    }
}

function Remove-ServiceForUpgrade([string] $Name) {
    $service = Get-Service -Name $Name -ErrorAction SilentlyContinue
    if (-not $service) {
        return
    }

    if ($service.Status -ne 'Stopped') {
        Stop-Service -Name $Name -Force -ErrorAction Stop
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }
    & sc.exe delete $Name | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Could not delete service $Name."
    }
    Wait-ServiceRemoved $Name
}

function Test-ZipEntryPaths([string] $Path) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        foreach ($entry in $archive.Entries) {
            $entryPath = $entry.FullName.Replace('\', '/')
            if ([IO.Path]::IsPathRooted($entryPath) -or $entryPath -match '(^|/)\.\.(/|$)') {
                throw "Bundle contains an unsafe ZIP entry path: $($entry.FullName)"
            }
        }
    } finally {
        $archive.Dispose()
    }
}

function Get-ScpExecutable {
    $scp = Get-Command scp.exe -CommandType Application -ErrorAction SilentlyContinue
    if (-not $scp) {
        $scp = Get-Command scp -CommandType Application -ErrorAction SilentlyContinue
    }
    if (-not $scp) {
        throw 'OpenSSH scp is not available. Install the Windows OpenSSH Client feature and rerun the installer.'
    }
    return $scp.Source
}

Assert-Elevated

$resolvedInstallationDirectory = Resolve-ConfigValue 'InstallationDirectory' $null (Join-Path $env:ProgramData 'GpgBridge')
$resolvedConfigPath = Resolve-ConfigValue 'ConfigPath' $null (Join-Path $resolvedInstallationDirectory 'state\install-config.json')
$existingConfig = Read-InstallerConfig $resolvedConfigPath

$resolvedInstallationDirectory = Resolve-ConfigValue 'InstallationDirectory' $existingConfig $resolvedInstallationDirectory
$resolvedConfigPath = Resolve-ConfigValue 'ConfigPath' $existingConfig $resolvedConfigPath
$BundleScpSource = Require-Value 'BundleScpSource' (Resolve-ConfigValue 'BundleScpSource' $existingConfig)
$AgentExtraSocket = Require-Value 'AgentExtraSocket' (Resolve-ConfigValue 'AgentExtraSocket' $existingConfig)
$configuredClientCaCert = Resolve-ConfigValue 'ClientCaCert' $existingConfig
$ListenAddress = Resolve-ConfigValue 'ListenAddress' $existingConfig
$TailscaleListenPort = Resolve-ConfigValue 'TailscaleListenPort' $existingConfig
$CaUrl = Require-Value 'CaUrl' (Resolve-ConfigValue 'CaUrl' $existingConfig)
$CaFingerprint = Require-Value 'CaFingerprint' (Resolve-ConfigValue 'CaFingerprint' $existingConfig)
$Provisioner = Require-Value 'Provisioner' (Resolve-ConfigValue 'Provisioner' $existingConfig)
$CommonName = Require-Value 'CommonName' (Resolve-ConfigValue 'CommonName' $existingConfig)
$DnsName = @(Resolve-ConfigValue 'DnsName' $existingConfig @())
$StepExecutable = Resolve-ConfigValue 'StepExecutable' $existingConfig
$MaxConnections = Resolve-ConfigValue 'MaxConnections' $existingConfig 64
$RenewalIntervalSeconds = Resolve-ConfigValue 'RenewalIntervalSeconds' $existingConfig 21600
$BridgeServiceName = Resolve-ConfigValue 'BridgeServiceName' $existingConfig 'gpg-bridge'

if ($CaUrl.Scheme -ne 'https') {
    throw 'CaUrl must use HTTPS.'
}
if ($CaFingerprint -notmatch '^[A-Fa-f0-9]{64}$') {
    throw 'CaFingerprint must be a 64-character hexadecimal SHA-256 fingerprint.'
}
$hasListenAddress = -not [string]::IsNullOrWhiteSpace($ListenAddress)
$hasTailscaleListenPort = $null -ne $TailscaleListenPort
if ($hasListenAddress -eq $hasTailscaleListenPort) {
    throw 'Specify exactly one of -ListenAddress or -TailscaleListenPort.'
}
if ($TailscaleListenPort -and ($TailscaleListenPort -lt 1 -or $TailscaleListenPort -gt 65535)) {
    throw 'TailscaleListenPort must be between 1 and 65535.'
}
$bundleDirectory = Join-Path $resolvedInstallationDirectory 'bundle'
$stateDirectory = Join-Path $resolvedInstallationDirectory 'state'
$certificateDirectory = Join-Path $stateDirectory 'certificates'
$logDirectory = Join-Path $stateDirectory 'logs'
$serverCert = Join-Path $certificateDirectory 'server-cert.pem'
$serverKey = Join-Path $certificateDirectory 'server-key.pem'
$rootCaCert = Join-Path $certificateDirectory 'root-ca.pem'
$bridgeLogPath = Join-Path $logDirectory 'gpg-bridge.log'
$renewalLogPath = Join-Path $logDirectory 'certificate-renewal.log'

New-Item -ItemType Directory -Path $stateDirectory, $certificateDirectory, $logDirectory -Force | Out-Null

$temporaryDirectory = Join-Path $env:TEMP ("gpg-bridge-install-" + [guid]::NewGuid().ToString('N'))
$temporaryZip = Join-Path $temporaryDirectory 'bundle.zip'
$stagedBundle = Join-Path $temporaryDirectory 'bundle'
$previousBundle = "$bundleDirectory.previous"
$bundleReplaced = $false
New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null

try {
    $scp = Get-ScpExecutable
    Write-Host "Fetching bundle from $BundleScpSource"
    & $scp $BundleScpSource $temporaryZip
    if ($LASTEXITCODE -ne 0) {
        throw "scp failed with exit code $LASTEXITCODE."
    }
    Assert-File 'Downloaded bundle' $temporaryZip
    Test-ZipEntryPaths $temporaryZip
    Expand-Archive -LiteralPath $temporaryZip -DestinationPath $stagedBundle

    foreach ($name in 'gpg-bridge.exe', 'install-service.ps1', 'install-certificate-renewal-service.ps1', 'request-step-ca-server-certificate.ps1', 'renew-step-ca-server-certificate.ps1') {
        Assert-File "Bundle file $name" (Join-Path $stagedBundle $name)
    }

    if (-not $Credential) {
        $Credential = Get-Credential -Message 'Enter the Windows account that owns the running Gpg4win agent.'
    }

    # The renewal service must stop first: it is permitted to restart the bridge.
    Remove-ServiceForUpgrade 'gpg-bridge-certificate-renewal'
    Remove-ServiceForUpgrade $BridgeServiceName

    if (Test-Path -LiteralPath $previousBundle) {
        Remove-Item -LiteralPath $previousBundle -Recurse -Force
    }
    if (Test-Path -LiteralPath $bundleDirectory) {
        Move-Item -LiteralPath $bundleDirectory -Destination $previousBundle
    }
    Move-Item -LiteralPath $stagedBundle -Destination $bundleDirectory
    $bundleReplaced = $true

    $serverCertExists = Test-Path -LiteralPath $serverCert -PathType Leaf
    $serverKeyExists = Test-Path -LiteralPath $serverKey -PathType Leaf
    if ($serverCertExists -ne $serverKeyExists) {
        throw "Server certificate state is incomplete in $certificateDirectory; restore or remove the server certificate and key before installing."
    }
    if ($serverCertExists -and -not (Test-Path -LiteralPath $rootCaCert -PathType Leaf)) {
        throw "The server certificate exists but its Step root is missing: $rootCaCert"
    }
    # A prior bootstrap can leave only root-ca.pem after a failed issuance. That
    # is safe to resume: request-step-ca-server-certificate.ps1 verifies and
    # preserves the matching pinned root before requesting the server keypair.
    if ($ReplaceCertificate -or -not $serverCertExists) {
        $requestArguments = @{
            CaUrl = $CaUrl
            CaFingerprint = $CaFingerprint
            Provisioner = $Provisioner
            CommonName = $CommonName
            OutputDirectory = $certificateDirectory
            PrivateKeyReadAccount = $Credential.UserName
        }
        if ($DnsName.Count -gt 0) {
            $requestArguments.DnsName = $DnsName
        }
        if ($ReplaceCertificate) {
            $requestArguments.Force = $true
        }
        & (Join-Path $bundleDirectory 'request-step-ca-server-certificate.ps1') @requestArguments
    }

    # A single Step root commonly signs both the serverAuth server certificate
    # and clientAuth Unix-client certificates. Use that trusted root unless a
    # separate client CA was explicitly configured.
    if ([string]::IsNullOrWhiteSpace($configuredClientCaCert)) {
        $ClientCaCert = $rootCaCert
    } else {
        $ClientCaCert = $configuredClientCaCert
    }
    Assert-File 'Client CA certificate' $ClientCaCert

    $executable = Join-Path $bundleDirectory 'gpg-bridge.exe'
    $bridgeArguments = @{
        Executable = $executable
        AgentExtraSocket = $AgentExtraSocket
        ClientCaCert = $ClientCaCert
        ServerCert = $serverCert
        ServerKey = $serverKey
        LogPath = $bridgeLogPath
        ServiceName = $BridgeServiceName
        MaxConnections = $MaxConnections
        Credential = $Credential
    }
    if ($TailscaleListenPort) {
        $bridgeArguments.TailscaleListenPort = $TailscaleListenPort
    } else {
        $bridgeArguments.ListenAddress = $ListenAddress
    }
    & (Join-Path $bundleDirectory 'install-service.ps1') @bridgeArguments

    $renewalArguments = @{
        Executable = $executable
        CaUrl = $CaUrl
        RootCaCert = $rootCaCert
        ServerCert = $serverCert
        ServerKey = $serverKey
        ExpectedDnsName = $CommonName
        RenewalScript = Join-Path $bundleDirectory 'renew-step-ca-server-certificate.ps1'
        BridgeServiceName = $BridgeServiceName
        LogPath = $renewalLogPath
        RenewalIntervalSeconds = $RenewalIntervalSeconds
        Credential = $Credential
    }
    if (-not [string]::IsNullOrWhiteSpace($StepExecutable)) {
        $renewalArguments.StepExecutable = $StepExecutable
    }
    & (Join-Path $bundleDirectory 'install-certificate-renewal-service.ps1') @renewalArguments

    foreach ($serviceName in $BridgeServiceName, 'gpg-bridge-certificate-renewal') {
        $service = Get-Service -Name $serviceName -ErrorAction Stop
        if ($service.Status -ne 'Running') {
            throw "Service did not start: $serviceName"
        }
    }

    $config = [ordered]@{
        BundleScpSource = $BundleScpSource
        InstallationDirectory = $resolvedInstallationDirectory
        ConfigPath = $resolvedConfigPath
        AgentExtraSocket = $AgentExtraSocket
        ClientCaCert = $configuredClientCaCert
        ListenAddress = $ListenAddress
        TailscaleListenPort = $TailscaleListenPort
        CaUrl = $CaUrl.AbsoluteUri
        CaFingerprint = $CaFingerprint
        Provisioner = $Provisioner
        CommonName = $CommonName
        DnsName = @($DnsName)
        StepExecutable = $StepExecutable
        MaxConnections = $MaxConnections
        RenewalIntervalSeconds = $RenewalIntervalSeconds
        BridgeServiceName = $BridgeServiceName
    }
    $config | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath $resolvedConfigPath -Encoding utf8

    if (Test-Path -LiteralPath $previousBundle) {
        Remove-Item -LiteralPath $previousBundle -Recurse -Force
    }
    Write-Host "Installed and started $BridgeServiceName and gpg-bridge-certificate-renewal. Configuration: $resolvedConfigPath"
} catch {
    if ($bundleReplaced -and (Test-Path -LiteralPath $previousBundle)) {
        Remove-Item -LiteralPath $bundleDirectory -Recurse -Force -ErrorAction SilentlyContinue
        Move-Item -LiteralPath $previousBundle -Destination $bundleDirectory
        Write-Warning 'Installation failed; restored the previous bundle. Services remain stopped.'
    }
    throw
} finally {
    if (Test-Path -LiteralPath $temporaryDirectory) {
        Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force
    }
}

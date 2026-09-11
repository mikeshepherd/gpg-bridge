[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Executable,
    [Parameter(Mandatory)] [uri] $CaUrl,
    [Parameter(Mandatory)] [string] $RootCaCert,
    [Parameter(Mandatory)] [string] $ServerCert,
    [Parameter(Mandatory)] [string] $ServerKey,
    [Parameter(Mandatory)] [string] $ExpectedDnsName,
    [string] $RenewalScript = (Join-Path $PSScriptRoot 'renew-step-ca-server-certificate.ps1'),
    [string] $StepExecutable,
    [string] $BridgeServiceName = 'gpg-bridge',
    [string] $LogPath,
    [ValidateRange(60, 604800)] [int] $RenewalIntervalSeconds = 21600,
    [System.Management.Automation.PSCredential] $Credential
)

$ErrorActionPreference = 'Stop'
if ($CaUrl.Scheme -ne 'https') {
    throw 'CaUrl must use HTTPS.'
}
foreach ($path in $Executable, $RenewalScript, $RootCaCert, $ServerCert, $ServerKey) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required file does not exist: $path"
    }
}
if (-not $StepExecutable) {
    $step = Get-Command step -CommandType Application -ErrorAction SilentlyContinue
    if (-not $step) {
        throw 'Step CLI is not available. Install it with request-step-ca-server-certificate.ps1 or pass -StepExecutable with its absolute path.'
    }
    $StepExecutable = $step.Source
}
if (-not (Test-Path -LiteralPath $StepExecutable -PathType Leaf)) {
    throw "Step executable does not exist: $StepExecutable"
}
if (-not $LogPath) {
    $LogPath = Join-Path (Split-Path -Parent $ServerCert) 'certificate-renewal.log'
}

$serviceName = 'gpg-bridge-certificate-renewal'
if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
    throw "Service already exists: $serviceName"
}
if (-not (Get-Service -Name $BridgeServiceName -ErrorAction SilentlyContinue)) {
    throw "Bridge service does not exist: $BridgeServiceName"
}
if (-not $Credential) {
    $Credential = Get-Credential -Message 'Enter the same Windows account that owns the Gpg4win agent and gpg-bridge service.'
}

function Quote-ServiceArgument([string] $Value) {
    '"' + $Value.Replace('"', '\"') + '"'
}

function Grant-BridgeRestartRights([string] $Service, [string] $AccountName) {
    $account = New-Object -TypeName System.Security.Principal.NTAccount -ArgumentList $AccountName
    $sid = $account.Translate([Security.Principal.SecurityIdentifier]).Value
    $descriptor = @(& sc.exe sdshow $Service) | Where-Object { $_ -match '^D:' } | Select-Object -First 1
    if (-not $descriptor) {
        throw "Could not read the security descriptor for bridge service $Service."
    }
    # RP and WP grant only SERVICE_START and SERVICE_STOP to the renewal account.
    $grant = "(A;;RPWP;;;$sid)"
    $saclIndex = $descriptor.IndexOf('S:')
    if ($saclIndex -ge 0) {
        $descriptor = $descriptor.Insert($saclIndex, $grant)
    } else {
        $descriptor += $grant
    }
    & sc.exe sdset $Service $descriptor | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Could not grant certificate-renewal service start/stop rights on $Service."
    }
}

$arguments = @(
    'windows-renewal-service',
    '--renewal-script', $RenewalScript,
    '--step-executable', $StepExecutable,
    '--ca-url', $CaUrl.AbsoluteUri,
    '--root-ca-cert', $RootCaCert,
    '--server-cert', $ServerCert,
    '--server-key', $ServerKey,
    '--expected-dns-name', $ExpectedDnsName,
    '--bridge-service-name', $BridgeServiceName,
    '--log-path', $LogPath,
    '--renewal-interval-seconds', $RenewalIntervalSeconds
)
$quotedArguments = $arguments | ForEach-Object { Quote-ServiceArgument ([string] $_) }
$binaryPath = (Quote-ServiceArgument $Executable) + ' ' + ($quotedArguments -join ' ')

Grant-BridgeRestartRights -Service $BridgeServiceName -AccountName $Credential.UserName
New-Service -Name $serviceName -DisplayName 'GPG Bridge Certificate Renewal' -BinaryPathName $binaryPath -Credential $Credential -StartupType Automatic
Start-Service -Name $serviceName
Write-Host "Installed and started service $serviceName. Renewal diagnostics are written to $LogPath."

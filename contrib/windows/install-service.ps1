[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $Executable,
    [string] $ListenAddress,
    [int] $TailscaleListenPort,
    [Parameter(Mandatory)] [string] $AgentExtraSocket,
    [Parameter(Mandatory)] [string] $ClientCaCert,
    [Parameter(Mandatory)] [string] $ServerCert,
    [Parameter(Mandatory)] [string] $ServerKey,
    [string] $ServiceName = 'gpg-bridge',
    [int] $MaxConnections = 64,
    [System.Management.Automation.PSCredential] $Credential
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw "Executable does not exist: $Executable"
}
if ([string]::IsNullOrWhiteSpace($ListenAddress) -ne $PSBoundParameters.ContainsKey('TailscaleListenPort')) {
    throw 'Specify exactly one of -ListenAddress or -TailscaleListenPort.'
}
if ($PSBoundParameters.ContainsKey('TailscaleListenPort') -and ($TailscaleListenPort -lt 1 -or $TailscaleListenPort -gt 65535)) {
    throw '-TailscaleListenPort must be between 1 and 65535.'
}
if (Get-Service -Name $ServiceName -ErrorAction SilentlyContinue) {
    throw "Service already exists: $ServiceName"
}
if (-not $Credential) {
    $Credential = Get-Credential -Message 'Enter the Windows account that owns the running Gpg4win agent.'
}

function Quote-ServiceArgument([string] $Value) {
    '"' + $Value.Replace('"', '\"') + '"'
}

$arguments = @('windows-service')
if ($PSBoundParameters.ContainsKey('TailscaleListenPort')) {
    $arguments += '--tailscale-listen-port', $TailscaleListenPort
} else {
    $arguments += '--listen-address', $ListenAddress
}
$arguments += @(
    '--agent-extra-socket', $AgentExtraSocket,
    '--client-ca-cert', $ClientCaCert,
    '--server-cert', $ServerCert,
    '--server-key', $ServerKey,
    '--max-connections', $MaxConnections
)
$quotedArguments = $arguments | ForEach-Object { Quote-ServiceArgument ([string] $_) }
$binaryPath = (Quote-ServiceArgument $Executable) + ' ' + ($quotedArguments -join ' ')

New-Service -Name $ServiceName -DisplayName 'GPG Bridge' -BinaryPathName $binaryPath -Credential $Credential -StartupType Automatic
Start-Service -Name $ServiceName
Write-Host "Installed and started service $ServiceName."

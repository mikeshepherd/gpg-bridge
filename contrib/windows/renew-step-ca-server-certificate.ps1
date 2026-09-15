[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $StepExecutable,
    [Parameter(Mandatory)] [uri] $CaUrl,
    [Parameter(Mandatory)] [string] $RootCaCert,
    [Parameter(Mandatory)] [string] $ServerCert,
    [Parameter(Mandatory)] [string] $ServerKey,
    [Parameter(Mandatory)] [string] $ExpectedDnsName,
    [string] $BridgeServiceName = 'gpg-bridge',
    [Parameter(Mandatory)] [string] $LogPath
)

$ErrorActionPreference = 'Stop'

function Write-RenewalLog([ValidateSet('INFO', 'ERROR')] [string] $Level, [string] $Message) {
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $line = "$timestamp $Level [gpg-bridge-certificate-renewal-script] $Message"
    Add-Content -LiteralPath $LogPath -Value $line -Encoding utf8
    Write-Output $line
}

if ($CaUrl.Scheme -ne 'https') {
    throw 'CaUrl must use HTTPS.'
}
foreach ($path in $StepExecutable, $RootCaCert, $ServerCert, $ServerKey) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required file does not exist: $path"
    }
}
$logDirectory = Split-Path -Parent $LogPath
if (-not [string]::IsNullOrWhiteSpace($logDirectory)) {
    New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
}

$temporaryCert = "$ServerCert.renewed-$([guid]::NewGuid().ToString('N')).pem"
$exitCode = 0
try {
    Write-RenewalLog -Level 'INFO' -Message 'Requesting Step CA certificate renewal.'
    # Windows PowerShell 5.1 promotes redirected native stderr to ErrorRecord.
    # step writes its successful "certificate saved" message to stderr, so
    # temporarily allow that output and decide success from the process status.
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $renewalOutput = @(& $StepExecutable ca renew $ServerCert $ServerKey --out $temporaryCert --ca-url $CaUrl.AbsoluteUri --root $RootCaCert 2>&1)
        $renewalExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($renewalExitCode -ne 0) {
        $details = ($renewalOutput | ForEach-Object { $_.ToString().Trim() } | Where-Object { $_ }) -join ' '
        if ([string]::IsNullOrWhiteSpace($details)) {
            throw "step ca renew failed with exit code $renewalExitCode."
        }
        throw "step ca renew failed with exit code ${renewalExitCode}: $details"
    }

    & $StepExecutable certificate verify --roots $RootCaCert $temporaryCert
    if ($LASTEXITCODE -ne 0) {
        throw 'Renewed certificate did not verify against the configured root CA.'
    }
    $inspection = & $StepExecutable certificate inspect --format json $temporaryCert
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not inspect the renewed certificate.'
    }
    $certificate = ($inspection -join "`n") | ConvertFrom-Json
    if (-not $certificate.extensions.extended_key_usage.server_auth) {
        throw 'Renewed certificate does not include the serverAuth EKU required by gpg-bridge.'
    }
    if ($certificate.extensions.subject_alt_name.dns_names -notcontains $ExpectedDnsName) {
        throw "Renewed certificate does not include the expected DNS SAN: $ExpectedDnsName"
    }

    Move-Item -LiteralPath $temporaryCert -Destination $ServerCert -Force
    Write-RenewalLog -Level 'INFO' -Message 'Installed verified renewed certificate; restarting GPG Bridge.'
    Restart-Service -Name $BridgeServiceName
    Write-RenewalLog -Level 'INFO' -Message 'GPG Bridge restarted after certificate renewal.'
} catch {
    try {
        Write-RenewalLog -Level 'ERROR' -Message "Certificate renewal failed: $($_.Exception.Message)"
    } catch {
        Write-Error "Certificate renewal failed and could not be logged: $($_.Exception.Message)"
    }
    $exitCode = 1
} finally {
    if (Test-Path -LiteralPath $temporaryCert) {
        Remove-Item -LiteralPath $temporaryCert -Force
    }
}

exit $exitCode

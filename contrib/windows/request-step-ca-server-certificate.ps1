[CmdletBinding()]
param(
    [Parameter(Mandatory)] [uri] $CaUrl,
    [Parameter(Mandatory)] [ValidatePattern('^[A-Fa-f0-9]{64}$')] [string] $CaFingerprint,
    [Parameter(Mandatory)] [string] $Provisioner,
    [Parameter(Mandatory)] [string] $CommonName,
    [Parameter(Mandatory)] [string] $OutputDirectory,
    [string[]] $DnsName = @(),
    [string] $PrivateKeyReadAccount = [Security.Principal.WindowsIdentity]::GetCurrent().Name,
    [switch] $Force
)

$ErrorActionPreference = 'Stop'
if ($CaUrl.Scheme -ne 'https') {
    throw 'CaUrl must use HTTPS.'
}

$step = Get-Command step -ErrorAction SilentlyContinue
if (-not $step) {
    $answer = Read-Host 'The Step CLI is missing. Install Smallstep.step with winget? [y/N]'
    if ($answer -notmatch '^(?i:y|yes)$') {
        throw 'Step CLI installation was declined.'
    }
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw 'winget is required to install the Step CLI.'
    }
    & winget install --exact --id Smallstep.step --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) {
        throw "winget failed with exit code $LASTEXITCODE."
    }
    $step = Get-Command step -ErrorAction SilentlyContinue
    if (-not $step) {
        throw 'Step CLI was installed but is not on this PowerShell PATH. Open a new PowerShell session and rerun the script.'
    }
}

function Get-StepFingerprint([string] $Path) {
    $fingerprint = (& step certificate fingerprint $Path).Trim().ToLowerInvariant()
    if ($LASTEXITCODE -ne 0) {
        throw "Could not fingerprint certificate: $Path"
    }
    $fingerprint
}

function Test-RootTrusted([string] $Fingerprint) {
    foreach ($store in 'Cert:\CurrentUser\Root', 'Cert:\LocalMachine\Root') {
        foreach ($certificate in @(Get-ChildItem -Path $store -ErrorAction SilentlyContinue)) {
            $sha256 = [System.Security.Cryptography.SHA256]::Create()
            try {
                $storedFingerprint = ([BitConverter]::ToString($sha256.ComputeHash($certificate.RawData))).Replace('-', '').ToLowerInvariant()
            } finally {
                $sha256.Dispose()
            }
            if ($storedFingerprint -eq $Fingerprint) {
                return $true
            }
        }
    }
    $false
}

$certificatePath = Join-Path $OutputDirectory 'server-cert.pem'
$keyPath = Join-Path $OutputDirectory 'server-key.pem'
$rootCopyPath = Join-Path $OutputDirectory 'root-ca.pem'
if ((Test-Path -LiteralPath $certificatePath) -or (Test-Path -LiteralPath $keyPath)) {
    if (-not $Force) {
        throw "Certificate or key already exists in $OutputDirectory. Use -Force only after verifying replacement is intended."
    }
}
$stepPath = (& step path).Trim()
$rootPath = Join-Path $stepPath 'certs\root_ca.crt'
$expectedFingerprint = $CaFingerprint.ToLowerInvariant()
if (Test-Path -LiteralPath $rootPath -PathType Leaf) {
    if ((Get-StepFingerprint $rootPath) -ne $expectedFingerprint) {
        throw "The existing Step root does not match the supplied CA fingerprint; refusing to replace it: $rootPath"
    }
    if (Test-RootTrusted $expectedFingerprint) {
        Write-Host 'A matching Step root and Windows trust-store entry already exist; preserving both.'
    } else {
        & step certificate install $rootPath
        if ($LASTEXITCODE -ne 0) {
            throw "Could not install the existing matching root into Windows trust."
        }
    }
} else {
    # The fingerprint pins the downloaded root before it is installed into Windows trust.
    & step ca bootstrap --ca-url $CaUrl.AbsoluteUri --fingerprint $CaFingerprint --install
    if ($LASTEXITCODE -ne 0) {
        throw "step ca bootstrap failed with exit code $LASTEXITCODE."
    }
    if (-not (Test-Path -LiteralPath $rootPath -PathType Leaf)) {
        throw "Step bootstrap did not create the expected root certificate: $rootPath"
    }
}
if ((Get-StepFingerprint $rootPath) -ne $expectedFingerprint) {
    throw "The Step root does not match the supplied CA fingerprint: $rootPath"
}
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
if (Test-Path -LiteralPath $rootCopyPath -PathType Leaf) {
    if ((Get-StepFingerprint $rootCopyPath) -eq $expectedFingerprint) {
        Write-Host "A matching output root already exists; preserving $rootCopyPath."
    } elseif ($Force) {
        Copy-Item -LiteralPath $rootPath -Destination $rootCopyPath -Force
    } else {
        throw "Output root differs from the supplied CA fingerprint; refusing to replace it: $rootCopyPath"
    }
} else {
    Copy-Item -LiteralPath $rootPath -Destination $rootCopyPath
}

$sans = @($CommonName) + $DnsName | Select-Object -Unique
$arguments = @('ca', 'certificate', $CommonName, $certificatePath, $keyPath, '--provisioner', $Provisioner, '--ca-url', $CaUrl.AbsoluteUri, '--root', $rootPath)
foreach ($san in $sans) {
    $arguments += @('--san', $san)
}
& step @arguments
if ($LASTEXITCODE -ne 0) {
    throw "step ca certificate failed with exit code $LASTEXITCODE."
}
& step certificate verify --roots $rootPath $certificatePath
if ($LASTEXITCODE -ne 0) {
    throw "Issued certificate did not verify against the bootstrapped root CA."
}

$inspection = & step certificate inspect --format json $certificatePath
if ($LASTEXITCODE -ne 0) {
    throw 'Could not inspect the issued certificate.'
}
$inspectionJson = ($inspection -join "`n") | ConvertFrom-Json
if (-not $inspectionJson.extensions.extended_key_usage.server_auth) {
    throw 'Issued certificate does not include the serverAuth EKU required by gpg-bridge.'
}

& icacls $keyPath /inheritance:r /grant:r "$($PrivateKeyReadAccount):R" 'SYSTEM:R' 'Administrators:R' | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "Could not restrict private-key ACLs; refusing to leave the key unprotected."
}
Write-Host "Created $certificatePath and $keyPath."
Write-Host "Use $rootCopyPath as the trusted CA input where appropriate."

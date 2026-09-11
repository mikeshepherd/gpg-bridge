# Certificates and keys

Use a private CA controlled by the operator. Issue a distinct client certificate for every Unix host. The examples use disposable placeholder paths.

## Create a CA and endpoint certificates

Run these commands on a protected administrative machine. Keep `ca-key.pem` offline; it is not copied to either bridge host.

```sh
umask 077
mkdir gpg-bridge-ca && cd gpg-bridge-ca
openssl genpkey -algorithm Ed25519 -out ca-key.pem
openssl req -x509 -new -key ca-key.pem -days 3650 -sha256 -subj '/CN=gpg-bridge private CA' -out ca-cert.pem
openssl genpkey -algorithm Ed25519 -out server-key.pem
openssl req -new -key server-key.pem -subj '/CN=windows-host.example' -out server.csr
openssl x509 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial -days 825 -sha256 -out server-cert.pem -extfile <(printf 'subjectAltName=DNS:windows-host.example\nextendedKeyUsage=serverAuth')
openssl genpkey -algorithm Ed25519 -out client-host-a-key.pem
openssl req -new -key client-host-a-key.pem -subj '/CN=client-host-a' -out client-host-a.csr
openssl x509 -req -in client-host-a.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial -days 825 -sha256 -out client-host-a-cert.pem -extfile <(printf 'extendedKeyUsage=clientAuth')
```

`genpkey` writes PKCS#8 PEM keys. If the client uses an IP address for `--server-name`, replace the server SAN with `subjectAltName=IP:192.0.2.10`; it must match exactly. The server certificate needs `serverAuth`; each client certificate needs `clientAuth`.

The examples use one CA for brevity. Separate server and client issuing CAs are also supported: give the Windows server the client CA and each Unix client the server CA.

## Distribution and permissions

| Machine | Files |
| --- | --- |
| Windows server | server certificate, server key, trusted client CA |
| Unix client | client certificate, client key, trusted server CA |
| CA host only | CA certificate and CA private key |

On Unix, private keys must be readable only by their owner; the program rejects group/other-readable key files:

```sh
install -d -m 700 "$HOME/.config/gpg-bridge"
chmod 600 "$HOME/.config/gpg-bridge"/*-key.pem
```

On Windows, store the server private key under the interactive user's profile and restrict its NTFS ACL to that user and required system administrators. Do not place private keys in shared folders or source control.

## Rotation and lost keys

Certificate material is loaded at process start. Replace files and restart the supervised service to rotate them. There is no CRL, OCSP, or client-certificate allowlist in this initial design. If a client private key is lost, issue a replacement and change the server's trusted client-CA material (normally by rotating to a new client CA) before restarting the Windows service. Deleting the old certificate alone does not revoke it.

## Windows Step CA server certificate

For a Step CA, use [request-step-ca-server-certificate.ps1](../contrib/windows/request-step-ca-server-certificate.ps1) from an elevated PowerShell session. It takes an HTTPS CA URL, the out-of-band verified 64-hex-character root fingerprint, provisioner name, server common name/SANs, and an output directory. If `step` is unavailable, it asks before installing `Smallstep.step` with winget.

The script first fingerprints any existing Step root. When it matches the supplied fingerprint, it preserves both that file and an existing matching Windows trust-store entry; it never deletes or replaces either. If the root is not already trusted, it installs that same existing root. Only when no Step root exists does it run fingerprint-pinned `step ca bootstrap --install`. It then uses Step's PEM-aware verification and inspection commands to validate the issued server-auth certificate. It grants the selected service account Modify access to the key so it can renew or replace certificate material, while SYSTEM and Administrators retain read-only access. It never embeds provisioner credentials; Step prompts according to the provisioner configuration. Pass `-PrivateKeyReadAccount` when the service account differs from the account running the script.

For a key created by an earlier archive, repair its ACL once from an elevated
PowerShell session before reissuing:

```powershell
icacls C:\Users\mikes\gpg-bridge\certificates\server-key.pem /inheritance:r /grant:r 'asus\mikes:M' 'SYSTEM:R' 'Administrators:R'
```

## Windows Step CA automatic renewal

For an intermittently connected Windows server, first apply the dedicated
provisioner policy in [the NixOS Step CA handoff](nixos-step-ca-expired-renewal-handoff.md).
Then install the `gpg-bridge-certificate-renewal` service from an elevated
PowerShell session. It runs as the same Windows account as Gpg4win and the
bridge service, renews every six hours by default, validates the replacement
chain/EKU/DNS SAN, and restarts `gpg-bridge` only after validation succeeds:

```powershell
.\install-certificate-renewal-service.ps1 `
  -Executable C:\Users\mikes\gpg-bridge\gpg-bridge.exe `
  -CaUrl https://ca.example.internal `
  -RootCaCert C:\Users\mikes\gpg-bridge\certificates\root-ca.pem `
  -ServerCert C:\Users\mikes\gpg-bridge\certificates\server-cert.pem `
  -ServerKey C:\Users\mikes\gpg-bridge\certificates\server-key.pem `
  -ExpectedDnsName asus.tail20bbc.ts.net
```

The installer resolves `step.exe` to an absolute path so the service does not
depend on a user PATH. It prompts for service-account credentials unless they
are supplied; use the same account as the bridge service. It grants that
account only service-start and service-stop rights on `gpg-bridge`, enabling a
renewal to reload the certificate without broader service-administration
permission. Renewal messages and failures are appended to
`certificate-renewal.log` beside the server certificate. A failed renewal
leaves the current certificate and running bridge service unchanged; the
renewal service retries at its next interval. The service can renew an expired
certificate only when its Step CA provisioner permits renewal after expiry.

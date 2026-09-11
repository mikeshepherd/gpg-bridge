# NixOS handoff: renew an intermittently connected GPG Bridge server certificate

## Outcome

Permit the Windows GPG Bridge server certificate for
`asus.tail20bbc.ts.net` to be renewed after it has expired. The change must be
limited to a new, dedicated Step CA provisioner; do not enable this policy on a
general-purpose provisioner or at CA-global scope.

The Windows host will subsequently run `step ca renew` using its existing
certificate and private key. It will not receive the provisioner's JWK private
key or password.

## Why this is needed

The CA limits certificates to 24 hours. Normal renewal needs a valid current
certificate, so a laptop or Windows host that is offline for longer than 24
hours cannot recover automatically. Step CA's
`allowRenewalAfterExpiry` claim is designed for intermittently connected
devices.

This intentionally changes the security posture: anyone with a copy of the
bridge server private key and its expired certificate can keep renewing it
until that certificate is revoked. Therefore, this must be narrowly scoped and
certificate revocation must be available to the CA operator.

## Scope and prerequisites

- Step CA configuration is declared in NixOS. Nix configuration, not `step ca
  provisioner update`, is the source of truth.
- The CA must have persistent certificate/revocation storage; a restart must
  not discard revocations.
- Retain the current 24-hour maximum and default duration. This proposal does
  not weaken the certificate lifetime policy.
- The first certificate must still be issued interactively from a trusted
  administrator workstation using the dedicated provisioner. Do not place JWK
  provisioner credentials on the Windows host.
- The issued certificate must retain the DNS SAN `asus.tail20bbc.ts.net` and
  both `serverAuth` and `clientAuth` EKUs. GPG Bridge verifies `serverAuth`
  during TLS connection; Step uses `clientAuth` for mTLS certificate renewal.

## NixOS implementation

Locate the Nix expression that produces the Step CA `authority.provisioners`
configuration. Add a new provisioner there, with a distinct name such as
`gpg-bridge-windows-server`. Use a separate JWK key from all existing
provisioners.

The resulting rendered Step CA provisioner must have this semantic shape:

```json
{
  "type": "JWK",
  "name": "gpg-bridge-windows-server",
  "key": "<public JWK only>",
  "encryptedKey": "<CA-managed encrypted JWK private key>",
  "claims": {
    "minTLSCertDuration": "5m",
    "defaultTLSCertDuration": "24h",
    "maxTLSCertDuration": "24h",
    "disableRenewal": false,
    "allowRenewalAfterExpiry": true
  }
}
```

Adapt the Nix attribute names to the module or configuration generator in use;
the important rendered `ca.json` property is
`claims.allowRenewalAfterExpiry: true`. Keep this claim absent or false for
all other provisioners.

If the current NixOS deployment uses a generated `ca.json`, generate the JWK
once through the deployment's existing secret-management method and store the
private/encrypted part only in the CA's protected secret store. Never commit it
to the Nix store or this repository. The public JWK may appear in the rendered
configuration.

### Limit what the provisioner can issue

A separate provisioner isolates the renewal-after-expiry capability, but a JWK
provisioner credential can otherwise request certificates within its policy.
Apply the deployment's existing X.509 template, webhook, or authorization
mechanism so initial issuance is limited to exactly:

- DNS SAN: `asus.tail20bbc.ts.net`
- intended server subject/CN
- both `serverAuth` and `clientAuth` EKUs
- no unrelated DNS/IP SANs

The renewal flow should preserve those identity properties. Do not give the
Windows machine the JWK credential merely to make initial issuance unattended.

## Deploy and verify

1. Evaluate and deploy the modified NixOS configuration using the normal
   host-deployment workflow.
2. Restart or reload `step-ca` as required by that deployment. A direct
   `step ca provisioner update` would create configuration drift and should not
   be used.
3. Inspect the live/rendered CA configuration on the CA host and confirm only
   `gpg-bridge-windows-server` has `allowRenewalAfterExpiry: true`.
4. From a trusted administrator workstation, issue a replacement bridge server
   certificate through the new provisioner and install it on Windows. Confirm:

   ```powershell
   step certificate verify --roots C:\Users\mikes\gpg-bridge\certificates\root-ca.pem C:\Users\mikes\gpg-bridge\certificates\server-cert.pem
   step certificate inspect --format json C:\Users\mikes\gpg-bridge\certificates\server-cert.pem
   ```

   Verify the inspection output has DNS SAN `asus.tail20bbc.ts.net`,
   `extended_key_usage.server_auth: true`, and
   `extended_key_usage.client_auth: true`.
5. Test normal renewal while it remains valid:

   ```powershell
   step ca renew --force --ca-url <CA-URL> --root C:\Users\mikes\gpg-bridge\certificates\root-ca.pem C:\Users\mikes\gpg-bridge\certificates\server-cert.pem C:\Users\mikes\gpg-bridge\certificates\server-key.pem
   ```

6. In a non-production test certificate or after waiting for expiry, run the
   same command after expiry. It must succeed only for this provisioner's
   certificate. Confirm a certificate issued by another provisioner remains
   unable to renew after expiry.
7. Restart GPG Bridge and perform a client signing operation. This proves both
   the renewed certificate and GPG Agent forwarding path work.

## Revocation and incident response

When the Windows server key may be exposed, revoke its certificate in Step CA
immediately and replace the key pair. Do not rely on expiry: this provisioner
explicitly permits post-expiry renewal. Confirm that a subsequent `step ca
renew` using the compromised key is rejected. Reissue with a new key and
certificate only after the host is trustworthy again.

Removing `allowRenewalAfterExpiry` is a rollback for future renewal attempts,
but does not revoke an already issued certificate. Use it only alongside normal
certificate revocation where compromise is suspected.

## References

- [Smallstep: certificate renewal options](https://smallstep.com/docs/step-ca/renewal/)
- [Smallstep: Step CA configuration claims](https://smallstep.com/docs/step-ca/configuration/)
- [Smallstep: `step ca renew`](https://smallstep.com/docs/step-cli/reference/ca/renew/)

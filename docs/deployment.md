# Deployment and operations

## Windows server

Run `gpg-bridge server` in the interactive Windows user's session. Gpg4win's agent and pinentry belong to that session, so a machine-wide service is not the initial deployment model. Configure Task Scheduler manually with an **At log on** trigger for that user, the complete `server` command, and restart on failure. Do not create the task automatically from this repository.

Allow only the selected TCP port in Windows Firewall and restrict permitted source networks where possible. mTLS protects authenticated traffic from eavesdropping and MITM only when the client verifies the configured server name and both CA chains are correct; firewall rules still reduce DoS exposure.

The `--agent-extra-socket` value is Gpg4win's redirection file for the restricted extra socket. The server reloads it for each new session, so a later request can recover after a Gpg4win agent restart.

### Windows service wrapper

`gpg-bridge windows-service` is a native Service Control Manager host around
the ordinary Windows server backend. It accepts SCM Stop controls and drains
the bridge using the normal shutdown path. It does not start Gpg4win or manage
pinentry.

Run [install-service.ps1](../contrib/windows/install-service.ps1) manually in
an elevated PowerShell session. It prompts for the Windows account that owns
the already-running Gpg4win agent, then creates a service with automatic
startup. Supply explicit absolute paths for the executable, redirect file,
certificate material, and key. The installer writes service messages to
`gpg-bridge.log` alongside the executable; override this with `-LogPath` when
needed. Do not run it as `LocalSystem`: that account
will normally be unable to read the interactive user's redirect file.

By default, provide `-ListenAddress <IP:PORT>` and the server binds exactly
that address. Alternatively, `-TailscaleListenPort <PORT>` adds
`--tailscale-listen-port` to the service. At each service start, this invokes
`tailscale ip -4` to find the one active Tailscale IPv4 address and binds it
to that port. The executable first uses the standard
`C:\Program Files\Tailscale\tailscale.exe` installation, then falls back to
`tailscale.exe` on `PATH`. Discovery times out after ten seconds rather than
leaving the service pending indefinitely. It does not rebind while running; restart the
service after a Tailscale address change.

To remove the manually installed service, first stop it and then delete it:

```powershell
Stop-Service -Name gpg-bridge
sc.exe delete gpg-bridge
```

If Step CA certificate automation is installed, stop and delete its separate
renewal service too:

```powershell
Stop-Service -Name gpg-bridge-certificate-renewal
sc.exe delete gpg-bridge-certificate-renewal
```

An SCM process runs outside the interactive desktop, but the bridge only needs
the redirect-file read permission and loopback connection to the already
running agent. It never starts `gpg-agent`: doing so from an SCM process would
create a Session 0 agent that cannot display pinentry. Any pinentry dialog
remains the Gpg4win agent's responsibility in the logged-in user's session.
Ensure Gpg4win starts the agent at user logon; if the agent is unavailable, the
bridge retries the redirect-file connection for fifteen seconds and then logs
the failure for that client session.

### Windows server installer and upgrades

[install-server.ps1](../contrib/windows/install-server.ps1) installs a complete
Windows server from an SCP-hosted Windows ZIP bundle. Run it from an elevated
PowerShell session while logged in as the user whose Gpg4win agent owns the
redirect file. The first run downloads and validates the ZIP, prompts once for
that user's service credential, obtains the initial Step CA certificate, and
starts both `gpg-bridge` and `gpg-bridge-certificate-renewal`.

It uses the existing Windows OpenSSH `scp` client and its normal SSH
configuration and authentication. The SCP source and all non-secret settings
are written to `C:\ProgramData\GpgBridge\state\install-config.json`; the
credential is never written to disk. Before running the command, ensure the
account already has a running Gpg4win agent. By default the installer uses the
fingerprint-pinned Step root it bootstraps as the trusted client CA; pass
`-ClientCaCert` only when Unix client certificates use a separate CA.

```powershell
.\install-server.ps1 `
  -BundleScpSource 'deploy@build-host:/srv/releases/gpg-bridge-windows.zip' `
  -AgentExtraSocket 'C:\Users\operator\AppData\Roaming\gnupg\S.gpg-agent.extra' `
  -ListenAddress '0.0.0.0:4321' `
  -CaUrl 'https://ca.example.internal' `
  -CaFingerprint '<verified-64-hex-character-root-fingerprint>' `
  -Provisioner 'windows-server' `
  -CommonName 'windows-host.example.internal' `
  -DnsName 'windows-host.example.internal'
```

Use `-TailscaleListenPort 4321` instead of `-ListenAddress` to use the
Tailscale address-discovery mode. The Step CLI installation prompt and any
provisioner authentication remain interactive; neither secret is persisted.

To update the executable and bundled scripts from the configured SCP source,
run the installer again with no configuration arguments:

```powershell
.\install-server.ps1
```

An update downloads and validates the new ZIP before stopping services. It
then stops and deletes the renewal service followed by the bridge service,
replaces only `C:\ProgramData\GpgBridge\bundle`, and reinstalls both services.
Certificates, logs, and configuration remain under `state` and are preserved.
Pass `-ReplaceCertificate` only when a new initial certificate is intended;
ordinary upgrades retain the existing certificate and key.

## Unix server backend

For Linux testing or a Unix server deployment, use `server --agent-socket <path>` with a local GnuPG restricted extra socket. This backend opens that Unix socket directly and does not send a Gpg4win nonce. It is mutually exclusive with Windows-only `--agent-extra-socket`; use one backend per server process.

## Unix client

Stop or reconfigure the local `gpg-agent` before the bridge starts: it must not own the same `--listen-socket`. The socket parent directory must exist. The remote GnuPG home needs public keys used for normal GnuPG operations, but must not receive the private signing/decryption key held on Windows.

Install the sample user unit as a template, provide its environment file with the endpoint and certificate values, then enable it manually:

```sh
systemctl --user daemon-reload
systemctl --user enable --now gpg-bridge-client.service
journalctl --user -u gpg-bridge-client.service -f
```

The service restarts after failure. A failed active operation remains failed by design; later GPG operations use fresh sessions automatically.

## Upgrade, rollback, and health checks

Before an upgrade, retain the previous executable and service configuration. Install the new executable, restart one side at a time, and make a real GPG operation that requires Windows pinentry. On failure, restore the prior executable/configuration and restart the supervised service.

Useful checks are Task Scheduler history and Windows logs on the server, plus `systemctl --user status gpg-bridge-client.service` and the user journal on Unix. For failures, verify DNS/address reachability, system clock validity, certificate SAN/EKU/CA trust, key permissions, socket ownership, and that the Windows user session and Gpg4win agent are running.

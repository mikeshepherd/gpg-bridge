# Deployment and operations

## Windows server

Run `gpg-bridge server` in the interactive Windows user's session. Gpg4win's agent and pinentry belong to that session, so a machine-wide service is not the initial deployment model. Configure Task Scheduler manually with an **At log on** trigger for that user, the complete `server` command, and restart on failure. Do not create the task automatically from this repository.

Allow only the selected TCP port in Windows Firewall and restrict permitted source networks where possible. mTLS protects authenticated traffic from eavesdropping and MITM only when the client verifies the configured server name and both CA chains are correct; firewall rules still reduce DoS exposure.

The `--agent-extra-socket` value is Gpg4win's redirection file for the restricted extra socket. The server reloads it for each new session, so a later request can recover after a Gpg4win agent restart.

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

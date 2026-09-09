# gpg-bridge

`gpg-bridge` lets a remote Unix host use the restricted Gpg4win extra socket belonging to an interactive Windows user.

```text
remote GnuPG -> owner-only Unix socket -> mTLS -> Windows bridge -> Gpg4win agent
```

The Windows `server` accepts TLS 1.3 clients authenticated by its client CA. The Unix `client` verifies the Windows certificate name and presents its own client certificate. Traffic is opaque GnuPG/Assuan bytes; it is never logged or replayed.

## Supported roles

- `server`: Windows, in the interactive user's session where Gpg4win and pinentry run; or Unix with a local GnuPG extra socket for testing and compatible deployments.
- `client`: Unix only, where it owns the local Unix-domain socket used by remote GnuPG.

See [certificate guidance](docs/certificates.md) and [deployment guidance](docs/deployment.md) before deploying.

For an automatic Windows background startup under the Gpg4win user's account,
see the documented Windows Service Control Manager wrapper.

Build a Windows ZIP containing `gpg-bridge.exe` and the Windows installation
and Step CA certificate scripts with `nix build .#windows-archive` on an
`x86_64-linux` host.

## Security and recovery

There is no plaintext, anonymous-client, or certificate-verification-bypass mode. Correct server-name verification and both certificate chains are essential for confidentiality and MITM protection. Restrict the Windows firewall to expected client networks as well: mTLS does not prevent DoS.

The services survive endpoint outages. While establishing a fresh local GPG session, the client retries transient network failures for up to 60 seconds. After any relay bytes may have crossed a connection, a disconnect fails that local session closed; the next GPG connection starts a new TLS session. An in-flight Assuan session is never replayed or transparently resumed.

## Build and verification

Nix with flakes enabled provides the development environment:

```sh
nix develop --command cargo fmt --all --check
nix develop --command cargo test --workspace --all-targets
nix develop --command cargo clippy --workspace --all-targets -- -D warnings
nix develop --command cargo check --workspace --target x86_64-pc-windows-gnu
```

## Quick start

Create separate server and client certificates signed by the appropriate trusted CA; do not copy a private key between hosts. The commands below use placeholders—see the certificate guide for generation and storage.

On Windows, while logged in as the Gpg4win user:

```text
gpg-bridge server \
  --listen-address 0.0.0.0:4321 \
  --agent-extra-socket C:\\path\\to\\S.gpg-agent.extra \
  --client-ca-cert C:\\path\\to\\client-ca.pem \
  --server-cert C:\\path\\to\\server-cert.pem \
  --server-key C:\\path\\to\\server-key.pem \
  --max-connections 64
```

On each remote Unix host, after ensuring no local `gpg-agent` owns the target socket:

```sh
gpg-bridge client \
  --listen-socket /run/user/$(id -u)/gnupg/S.gpg-agent \
  --server-address windows-host.example:4321 \
  --server-name windows-host.example \
  --server-ca-cert "$HOME/.config/gpg-bridge/server-ca.pem" \
  --client-cert "$HOME/.config/gpg-bridge/client-cert.pem" \
  --client-key "$HOME/.config/gpg-bridge/client-key.pem" \
  --max-connections 64
```

The Unix socket parent directory must already exist. The client refuses to replace a live socket, symlink, regular file, or directory, and applies mode `0600` to the socket it owns.

For same-machine Linux testing, run the server against a separate local GnuPG extra socket. Unlike the Windows backend, this direct Unix-socket backend does not use Gpg4win redirect metadata or a nonce:

```sh
gpg-bridge server \
  --listen-address 127.0.0.1:4321 \
  --agent-socket "$HOME/.gnupg/S.gpg-agent.extra" \
  --client-ca-cert "$HOME/.config/gpg-bridge/client-ca.pem" \
  --server-cert "$HOME/.config/gpg-bridge/server-cert.pem" \
  --server-key "$HOME/.config/gpg-bridge/server-key.pem"
```

For a disposable local mTLS setup around an existing Linux extra socket, run:

```sh
nix develop --command scripts/local-smoke.sh \
  --agent-socket "$HOME/.gnupg/S.gpg-agent.extra"
```

It leaves the agent untouched, prints a `gpg-connect-agent` command for manual testing, and cleans up the bridge processes and temporary certificates on exit. Add `--smoke` to run a harmless `/bye` probe and exit automatically.

## License

This project is licensed under the MIT License.

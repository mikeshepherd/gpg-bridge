# gpg-bridge

`gpg-bridge` exposes a Gpg4win GnuPG agent redirection socket through a TCP
listener. It is intended for use with a protected SSH tunnel so a remote Unix
machine can reach the Windows user's GPG agent.

## Security status

**The TCP transport is currently plaintext and does not authenticate peers. Do
not expose it directly to an untrusted network.** Use a protected tunnel such
as the SSH reverse forward described below.

Direct mutual-TLS connectivity, automatic reconnect handling, and a companion
Unix-socket client are planned but are not implemented yet.

## Prerequisites

- [Nix](https://nixos.org/download/) with flakes enabled for development.
- Gpg4win/GnuPG running for the interactive Windows user.
- Access to that user's GPG agent extra-socket redirection file.

## Development

Enter the development shell, then run the usual checks:

```sh
nix develop
cargo fmt --all --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace
cargo check --workspace --target x86_64-pc-windows-gnu
```

## Current usage

Run the bridge on the Windows host, replacing the placeholders with the TCP
address to listen on and the Gpg4win extra-socket redirection-file path:

```sh
cargo run -- gpg-bridge \
  --extra <listen-address:port> \
  --extra-socket <gpg4win-extra-socket-redirection-file>
```

The bridge reads the redirect file, connects to its local loopback TCP port,
performs the Gpg4win nonce exchange, and relays traffic between that connection
and each accepted TCP stream.

The current remote shape is an SSH reverse Unix-socket-to-TCP forward:

```text
remote GnuPG -> remote Unix socket -> SSH reverse forward
    -> Windows gpg-bridge TCP listener -> Gpg4win agent extra socket
```

For example, configure an SSH reverse forward so a Unix-domain socket on the
remote host forwards to the bridge's loopback TCP listener on the Windows host.
The socket path must be the one used by the remote GnuPG client, and the remote
SSH server must permit remote Unix-socket forwarding. Stop or reconfigure any
remote `gpg-agent` that already owns that socket before creating the forward.

## License

This project is licensed under the MIT License.

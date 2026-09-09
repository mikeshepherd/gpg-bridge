#!/usr/bin/env bash
# Run a disposable local mTLS bridge around an existing Unix GnuPG extra socket.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/local-smoke.sh --agent-socket PATH [--listen-address ADDRESS] [--smoke]

Starts a local gpg-bridge server and client with disposable certificates.
PATH must be an existing Unix GnuPG restricted extra socket. The script never
starts, stops, or removes that agent or socket.

Options:
  --agent-socket PATH       Existing local GnuPG extra socket (required)
  --listen-address ADDRESS  Local TCP address (default: 127.0.0.1:4321)
  --smoke                   Run gpg-connect-agent /bye, then clean up and exit
  -h, --help                Show this help
EOF
}

agent_socket=''
listen_address='127.0.0.1:4321'
smoke=false
while (($#)); do
  case "$1" in
    --agent-socket) agent_socket=${2:?missing agent socket}; shift 2 ;;
    --listen-address) listen_address=${2:?missing listen address}; shift 2 ;;
    --smoke) smoke=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -z $agent_socket || ! -S $agent_socket ]]; then
  printf '%s\n' '--agent-socket must name an existing Unix socket' >&2
  exit 2
fi
if ! command -v openssl >/dev/null || ! command -v cargo >/dev/null; then
  printf '%s\n' 'Run this script through `nix develop --command scripts/local-smoke.sh ...`.' >&2
  exit 127
fi
if [[ $smoke == true ]] && ! command -v gpg-connect-agent >/dev/null; then
  printf '%s\n' 'gpg-connect-agent is required for --smoke.' >&2
  exit 127
fi

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/gpg-bridge-local.XXXXXX")
client_socket="$tmpdir/S.gpg-agent"
server_pid=''
client_pid=''
cleanup() {
  [[ -z $client_pid ]] || kill "$client_pid" 2>/dev/null || true
  [[ -z $server_pid ]] || kill "$server_pid" 2>/dev/null || true
  [[ -z $client_pid ]] || wait "$client_pid" 2>/dev/null || true
  [[ -z $server_pid ]] || wait "$server_pid" 2>/dev/null || true
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

umask 077
openssl genpkey -algorithm Ed25519 -out "$tmpdir/ca-key.pem"
openssl req -x509 -new -key "$tmpdir/ca-key.pem" -days 1 -sha256 \
  -subj '/CN=gpg-bridge local test CA' -out "$tmpdir/ca-cert.pem"
openssl genpkey -algorithm Ed25519 -out "$tmpdir/server-key.pem"
openssl req -new -key "$tmpdir/server-key.pem" -subj '/CN=localhost' -out "$tmpdir/server.csr"
openssl x509 -req -in "$tmpdir/server.csr" -CA "$tmpdir/ca-cert.pem" \
  -CAkey "$tmpdir/ca-key.pem" -CAcreateserial -days 1 -sha256 \
  -out "$tmpdir/server-cert.pem" \
  -extfile <(printf 'subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth')
openssl genpkey -algorithm Ed25519 -out "$tmpdir/client-key.pem"
openssl req -new -key "$tmpdir/client-key.pem" -subj '/CN=local-client' -out "$tmpdir/client.csr"
openssl x509 -req -in "$tmpdir/client.csr" -CA "$tmpdir/ca-cert.pem" \
  -CAkey "$tmpdir/ca-key.pem" -CAcreateserial -days 1 -sha256 \
  -out "$tmpdir/client-cert.pem" -extfile <(printf 'extendedKeyUsage=clientAuth')

cargo run -- server --listen-address "$listen_address" --agent-socket "$agent_socket" \
  --client-ca-cert "$tmpdir/ca-cert.pem" --server-cert "$tmpdir/server-cert.pem" \
  --server-key "$tmpdir/server-key.pem" &
server_pid=$!
cargo run -- client --listen-socket "$client_socket" --server-address "$listen_address" \
  --server-name localhost --server-ca-cert "$tmpdir/ca-cert.pem" \
  --client-cert "$tmpdir/client-cert.pem" --client-key "$tmpdir/client-key.pem" &
client_pid=$!

for _ in {1..100}; do
  [[ -S $client_socket ]] && break
  kill -0 "$server_pid" 2>/dev/null || { printf '%s\n' 'Server exited unexpectedly.' >&2; exit 1; }
  kill -0 "$client_pid" 2>/dev/null || { printf '%s\n' 'Client exited unexpectedly.' >&2; exit 1; }
  sleep 0.05
done
if [[ ! -S $client_socket ]]; then
  printf '%s\n' 'Client socket was not created.' >&2
  exit 1
fi

printf 'Bridge ready at %s\n' "$client_socket"
printf 'Exercise it with: gpg-connect-agent --raw-socket %q /bye\n' "$client_socket"
if [[ $smoke == true ]]; then
  gpg-connect-agent --raw-socket "$client_socket" /bye
  exit 0
fi
printf '%s\n' 'Press Ctrl-C to stop the bridge and remove temporary certificates.'
wait "$client_pid"

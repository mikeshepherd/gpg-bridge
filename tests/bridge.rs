#![cfg(unix)]

use std::fs;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpg_bridge::{
    ClientOptions, ServerAgent, ServerOptions, client::run_client_until, server::run_server_until,
};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixListener, UnixStream};
use tokio::sync::watch;

struct Materials {
    directory: TempDir,
    ca: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
}

fn write_private_key(path: &Path, key: &KeyPair) {
    fs::write(path, key.serialize_pem()).expect("write private key");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("secure key mode");
}

fn materials() -> Materials {
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().expect("CA key"))
        .expect("CA certificate");
    let mut server_params =
        CertificateParams::new(vec!["server.test".to_owned()]).expect("server parameters");
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_key = KeyPair::generate().expect("server key");
    let server = server_params
        .signed_by(&server_key, &ca)
        .expect("server certificate");
    let mut client_params = CertificateParams::default();
    client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client_key = KeyPair::generate().expect("client key");
    let client = client_params
        .signed_by(&client_key, &ca)
        .expect("client certificate");
    let ca_path = directory.path().join("ca.pem");
    let server_cert = directory.path().join("server.pem");
    let server_key_path = directory.path().join("server-key.pem");
    let client_cert = directory.path().join("client.pem");
    let client_key_path = directory.path().join("client-key.pem");
    fs::write(&ca_path, ca.pem()).expect("write CA");
    fs::write(&server_cert, server.pem()).expect("write server certificate");
    write_private_key(&server_key_path, &server_key);
    fs::write(&client_cert, client.pem()).expect("write client certificate");
    write_private_key(&client_key_path, &client_key);
    Materials {
        directory,
        ca: ca_path,
        server_cert,
        server_key: server_key_path,
        client_cert,
        client_key: client_key_path,
    }
}

fn unused_loopback_address() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("reserve port");
    let address = listener.local_addr().expect("reserved address");
    drop(listener);
    address
}

async fn wait_for_socket(path: &Path) {
    for _ in 0..50 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("Unix socket was not bound");
}

#[tokio::test]
async fn client_and_server_relay_binary_data_over_mutual_tls() {
    let materials = materials();
    let agent_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("agent listener");
    let redirect_file = materials.directory.path().join("S.gpg-agent.extra");
    let nonce = [9_u8; 16];
    let mut redirect = agent_listener
        .local_addr()
        .expect("agent address")
        .port()
        .to_string()
        .into_bytes();
    redirect.extend(nonce);
    fs::write(&redirect_file, redirect).expect("redirect metadata");
    let agent = tokio::spawn(async move {
        let (mut stream, _) = agent_listener.accept().await.expect("agent accepts bridge");
        let mut received_nonce = [0; 16];
        stream.read_exact(&mut received_nonce).await.expect("nonce");
        assert_eq!(received_nonce, nonce);
        let mut payload = [0; 5];
        stream.read_exact(&mut payload).await.expect("payload");
        assert_eq!(payload, [0, 255, 1, 2, 3]);
        stream
            .write_all(&[3, 2, 1, 255, 0])
            .await
            .expect("response");
    });
    let address = unused_loopback_address();
    let (server_sender, mut server_shutdown) = watch::channel(false);
    let server_options = ServerOptions::new(
        address,
        redirect_file,
        materials.ca.clone(),
        materials.server_cert.clone(),
        materials.server_key.clone(),
        4,
    )
    .expect("server options");
    let server =
        tokio::spawn(async move { run_server_until(server_options, &mut server_shutdown).await });
    let socket = materials.directory.path().join("gpg-agent.sock");
    let (client_sender, mut client_shutdown) = watch::channel(false);
    let client_options = ClientOptions::new(
        socket.clone(),
        address.to_string(),
        "server.test".to_owned(),
        materials.ca.clone(),
        materials.client_cert.clone(),
        materials.client_key.clone(),
        4,
    )
    .expect("client options");
    let client =
        tokio::spawn(async move { run_client_until(client_options, &mut client_shutdown).await });
    wait_for_socket(&socket).await;
    let mut local = UnixStream::connect(&socket)
        .await
        .expect("connect client socket");
    local.write_all(&[0, 255, 1, 2, 3]).await.expect("request");
    let mut reply = [0; 5];
    tokio::time::timeout(Duration::from_secs(5), local.read_exact(&mut reply))
        .await
        .expect("response timeout")
        .expect("response");
    assert_eq!(reply, [3, 2, 1, 255, 0]);
    drop(local);
    agent.await.expect("agent task");
    client_sender.send(true).expect("stop client");
    server_sender.send(true).expect("stop server");
    client.await.expect("client task").expect("client result");
    server.await.expect("server task").expect("server result");
}

#[tokio::test]
async fn client_and_server_relay_through_a_unix_agent_socket() {
    let materials = materials();
    let agent_path = materials.directory.path().join("S.gpg-agent.extra");
    let agent_listener = UnixListener::bind(&agent_path).expect("Unix agent listener");
    let agent = tokio::spawn(async move {
        let (mut stream, _) = agent_listener.accept().await.expect("agent accepts bridge");
        let mut payload = [0; 4];
        stream.read_exact(&mut payload).await.expect("payload");
        assert_eq!(payload, [0, 255, 1, 2]);
        stream.write_all(&[2, 1, 255, 0]).await.expect("response");
    });
    let address = unused_loopback_address();
    let (server_sender, mut server_shutdown) = watch::channel(false);
    let server_options = ServerOptions::new_with_agent(
        address,
        ServerAgent::UnixSocket(agent_path),
        materials.ca.clone(),
        materials.server_cert.clone(),
        materials.server_key.clone(),
        4,
    )
    .expect("server options");
    let server =
        tokio::spawn(async move { run_server_until(server_options, &mut server_shutdown).await });
    let socket = materials.directory.path().join("gpg-agent.sock");
    let (client_sender, mut client_shutdown) = watch::channel(false);
    let client_options = ClientOptions::new(
        socket.clone(),
        address.to_string(),
        "server.test".to_owned(),
        materials.ca.clone(),
        materials.client_cert.clone(),
        materials.client_key.clone(),
        4,
    )
    .expect("client options");
    let client =
        tokio::spawn(async move { run_client_until(client_options, &mut client_shutdown).await });
    wait_for_socket(&socket).await;
    let mut local = UnixStream::connect(&socket)
        .await
        .expect("connect client socket");
    local.write_all(&[0, 255, 1, 2]).await.expect("request");
    let mut reply = [0; 4];
    tokio::time::timeout(Duration::from_secs(5), local.read_exact(&mut reply))
        .await
        .expect("response timeout")
        .expect("response");
    assert_eq!(reply, [2, 1, 255, 0]);
    drop(local);
    agent.await.expect("agent task");
    client_sender.send(true).expect("stop client");
    server_sender.send(true).expect("stop server");
    client.await.expect("client task").expect("client result");
    server.await.expect("server task").expect("server result");
}

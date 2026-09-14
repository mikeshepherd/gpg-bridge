//! Windows Tailscale address discovery for the optional server listener mode.

use std::net::Ipv4Addr;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use std::net::{IpAddr, SocketAddr};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TailscaleError {
    #[error("Tailscale returned no IPv4 address")]
    MissingAddress,
    #[error("Tailscale returned more than one IPv4 address")]
    MultipleAddresses,
    #[error("Tailscale returned an invalid IPv4 address: {0}")]
    InvalidAddress(String),
    #[cfg(windows)]
    #[error("failed to run the Tailscale CLI: {0}")]
    Command(#[source] std::io::Error),
    #[cfg(windows)]
    #[error("Tailscale CLI failed: {0}")]
    Failed(String),
    #[cfg(windows)]
    #[error("Tailscale CLI did not respond within {0:?}")]
    Timeout(Duration),
}

#[cfg_attr(not(windows), allow(dead_code))]
fn parse_ipv4(output: &str) -> Result<Ipv4Addr, TailscaleError> {
    let addresses = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.parse::<Ipv4Addr>()
                .map_err(|_| TailscaleError::InvalidAddress(line.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    match addresses.as_slice() {
        [] => Err(TailscaleError::MissingAddress),
        [address] => Ok(*address),
        _ => Err(TailscaleError::MultipleAddresses),
    }
}

#[cfg(windows)]
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(windows)]
async fn run_tailscale(command_path: &str) -> Result<std::process::Output, TailscaleError> {
    let mut command = tokio::process::Command::new(command_path);
    command.args(["ip", "-4"]);
    command.kill_on_drop(true);
    tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| TailscaleError::Timeout(COMMAND_TIMEOUT))?
        .map_err(TailscaleError::Command)
}

#[cfg(windows)]
async fn tailscale_output() -> Result<std::process::Output, TailscaleError> {
    use std::io::ErrorKind;

    const DEFAULT_TAILSCALE_CLI: &str = r"C:\Program Files\Tailscale\tailscale.exe";
    match run_tailscale(DEFAULT_TAILSCALE_CLI).await {
        Ok(output) => Ok(output),
        Err(TailscaleError::Command(error)) if error.kind() == ErrorKind::NotFound => {
            run_tailscale("tailscale.exe").await
        }
        Err(error) => Err(error),
    }
}

/// Resolves the active Tailscale IPv4 address for a Windows listener.
///
/// # Errors
///
/// Returns an error when the Tailscale CLI cannot report exactly one IPv4
/// address.
#[cfg(windows)]
pub async fn listen_address(port: u16) -> Result<SocketAddr, TailscaleError> {
    let output = tailscale_output().await?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(TailscaleError::Failed(if error.is_empty() {
            format!("exited with {status}", status = output.status)
        } else {
            error
        }));
    }
    Ok(SocketAddr::new(
        IpAddr::V4(parse_ipv4(&String::from_utf8_lossy(&output.stdout))?),
        port,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_one_ipv4_address() {
        assert_eq!(
            parse_ipv4("100.75.96.114\n").expect("address parses"),
            Ipv4Addr::new(100, 75, 96, 114)
        );
    }

    #[test]
    fn rejects_missing_invalid_or_ambiguous_addresses() {
        assert!(matches!(
            parse_ipv4("\n"),
            Err(TailscaleError::MissingAddress)
        ));
        assert!(matches!(
            parse_ipv4("not-an-address\n"),
            Err(TailscaleError::InvalidAddress(_))
        ));
        assert!(matches!(
            parse_ipv4("100.64.0.1\n100.64.0.2\n"),
            Err(TailscaleError::MultipleAddresses)
        ));
    }
}

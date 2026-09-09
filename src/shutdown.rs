use std::io;

use tokio::sync::watch;

/// Waits for Ctrl-C on all platforms and SIGTERM on Unix.
///
/// # Errors
///
/// Returns an error if the platform signal handler cannot be registered or read.
pub async fn wait_for_shutdown() -> io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

/// Waits for shutdown and broadcasts cancellation to active tasks.
///
/// # Errors
///
/// Returns an error from the platform signal handler.
pub async fn notify_shutdown(sender: watch::Sender<bool>) -> io::Result<()> {
    wait_for_shutdown().await?;
    let _ = sender.send(true);
    Ok(())
}

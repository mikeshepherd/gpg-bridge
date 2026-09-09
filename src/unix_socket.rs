#![cfg(unix)]

use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

#[derive(Debug)]
pub struct OwnedUnixListener {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl OwnedUnixListener {
    /// Binds an owner-only socket, replacing only a demonstrably stale socket.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing parent, unsafe collision, stale-socket race,
    /// bind failure, or an unexpected resulting mode.
    pub async fn bind(path: PathBuf) -> io::Result<Self> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unix socket path has no parent",
            )
        })?;
        if !parent.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "Unix socket parent directory does not exist",
            ));
        }
        Self::remove_stale_socket(&path).await?;
        Self::bind_new(path)
    }

    fn bind_new(path: PathBuf) -> io::Result<Self> {
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Unix socket did not receive mode 0600",
            ));
        }
        Ok(Self {
            listener,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    async fn remove_stale_socket(path: &Path) -> io::Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                match UnixStream::connect(path).await {
                    Ok(_) => Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "Unix socket is owned by an active process",
                    )),
                    Err(error) if is_stale_socket_error(&error) => {
                        let current = std::fs::symlink_metadata(path)?;
                        if !current.file_type().is_socket()
                            || current.dev() != metadata.dev()
                            || current.ino() != metadata.ino()
                        {
                            return Err(io::Error::other(
                                "Unix socket changed while checking ownership",
                            ));
                        }
                        std::fs::remove_file(path)
                    }
                    Err(error) => Err(io::Error::new(
                        error.kind(),
                        format!("could not verify existing Unix socket ownership: {error}"),
                    )),
                }
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to replace a non-socket Unix path",
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Restores an unlinked owned path, without disturbing a replacement owner.
    ///
    /// # Errors
    ///
    /// Returns an error when a different owner replaces the path or rebinding fails.
    pub fn verify_or_rebind(&mut self) -> io::Result<()> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode =>
            {
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let replacement = Self::bind_new(self.path.clone())?;
                *self = replacement;
                Ok(())
            }
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "Unix socket path was replaced by another owner",
            )),
            Err(error) => Err(error),
        }
    }

    /// Accepts the next local `GnuPG` connection.
    ///
    /// # Errors
    ///
    /// Returns Unix listener acceptance errors.
    pub async fn accept(&self) -> io::Result<(UnixStream, tokio::net::unix::SocketAddr)> {
        self.listener.accept().await
    }
}

fn is_stale_socket_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
    )
}

impl Drop for OwnedUnixListener {
    fn drop(&mut self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path)
            && metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binds_owner_only_socket_and_cleans_up_its_own_path() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("agent.sock");
        let listener = OwnedUnixListener::bind(path.clone())
            .await
            .expect("bind socket");
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(listener);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn refuses_regular_file_and_symlink_collisions() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("agent.sock");
        std::fs::write(&path, "not a socket").expect("regular file");
        assert_eq!(
            OwnedUnixListener::bind(path.clone())
                .await
                .expect_err("reject file")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_file(&path).expect("remove test file");
        std::os::unix::fs::symlink(directory.path(), &path).expect("symlink");
        assert_eq!(
            OwnedUnixListener::bind(path)
                .await
                .expect_err("reject symlink")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[tokio::test]
    async fn rebinds_an_unlinked_owned_socket() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("agent.sock");
        let mut listener = OwnedUnixListener::bind(path.clone())
            .await
            .expect("bind socket");
        std::fs::remove_file(&path).expect("unlink socket");
        listener.verify_or_rebind().expect("rebind socket");
        assert!(path.exists());
    }
}

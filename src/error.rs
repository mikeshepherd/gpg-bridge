use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("a required path is empty")]
    EmptyPath,
    #[error("server address is empty")]
    EmptyServerAddress,
    #[error("server name is empty")]
    EmptyServerName,
    #[error("max connections must be greater than zero")]
    ZeroMaxConnections,
}

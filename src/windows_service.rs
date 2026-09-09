//! Windows Service Control Manager host for the bridge server.

use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use log::error;
use tokio::sync::watch;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::{ServerOptions, server};

const SERVICE_NAME: &str = "gpg-bridge";
static OPTIONS: OnceLock<ServiceOptions> = OnceLock::new();

#[derive(Clone)]
struct ServiceOptions {
    server: ServerOptions,
    tailscale_listen_port: Option<u16>,
}

define_windows_service!(ffi_service_main, service_main);

/// Starts the SCM dispatcher for the configured bridge server.
///
/// # Errors
///
/// Returns an error when called more than once or when SCM dispatch fails.
pub fn run(
    options: ServerOptions,
    tailscale_listen_port: Option<u16>,
) -> windows_service::Result<()> {
    OPTIONS
        .set(ServiceOptions {
            server: options,
            tailscale_listen_port,
        })
        .map_err(|_| {
            windows_service::Error::Winapi(std::io::Error::other("service already started"))
        })?;
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn service_main(_: Vec<std::ffi::OsString>) {
    if let Err(error) = run_service() {
        error!("Windows service failed: {error}");
    }
}

fn status(state: ServiceState, accepted: ServiceControlAccept) -> ServiceStatus {
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accepted,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::ZERO,
        process_id: None,
    }
}

fn run_service() -> windows_service::Result<()> {
    let service_options = OPTIONS.get().expect("service options initialized").clone();
    let mut options = service_options.server;
    let (stop_sender, stop_receiver) = mpsc::channel();
    let status_handle =
        service_control_handler::register(SERVICE_NAME, move |control| match control {
            ServiceControl::Stop => {
                let _ = stop_sender.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?;
    status_handle.set_service_status(status(
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
    ))?;
    if let Some(port) = service_options.tailscale_listen_port {
        options.listen_address = crate::tailscale::listen_address(port)
            .map_err(|error| windows_service::Error::Winapi(std::io::Error::other(error)))?;
    }
    status_handle.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP))?;

    let (shutdown_sender, mut shutdown_receiver) = watch::channel(false);
    std::thread::spawn(move || {
        let _ = stop_receiver.recv();
        let _ = shutdown_sender.send(true);
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(windows_service::Error::Winapi)?;
    if let Err(error) = runtime.block_on(server::run_server_until(options, &mut shutdown_receiver))
    {
        error!("bridge server terminated: {error}");
    }
    status_handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))
}

//! Windows Service Control Manager host for periodic Step CA certificate renewal.

use std::{
    ffi::OsString,
    process::{Child, Command},
    sync::{OnceLock, mpsc},
    time::Duration,
};

use log::{error, info, warn};
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

const SERVICE_NAME: &str = "gpg-bridge-certificate-renewal";
static OPTIONS: OnceLock<RenewalOptions> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct RenewalOptions {
    pub renewal_script: std::path::PathBuf,
    pub step_executable: std::path::PathBuf,
    pub ca_url: String,
    pub root_ca_cert: std::path::PathBuf,
    pub server_cert: std::path::PathBuf,
    pub server_key: std::path::PathBuf,
    pub expected_dns_name: String,
    pub bridge_service_name: String,
    pub log_path: std::path::PathBuf,
    pub renewal_interval: Duration,
}

define_windows_service!(ffi_service_main, service_main);

/// Starts the SCM dispatcher for the certificate-renewal service.
pub fn run(options: RenewalOptions) -> windows_service::Result<()> {
    OPTIONS.set(options).map_err(|_| {
        windows_service::Error::Winapi(std::io::Error::other("service already started"))
    })?;
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)
}

fn service_main(_: Vec<OsString>) {
    if let Err(error) = run_service() {
        error!("Windows certificate-renewal service failed: {error}");
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
    let options = OPTIONS.get().expect("service options initialized").clone();
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
    status_handle.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP))?;

    loop {
        match run_renewal(&options, &stop_receiver) {
            Ok(true) => info!("certificate renewal completed"),
            Ok(false) => break,
            Err(error) => warn!("certificate renewal failed: {error}"),
        }
        match stop_receiver.recv_timeout(options.renewal_interval) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }

    status_handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))
}

/// Returns `false` if SCM requested a stop while the child process was active.
fn run_renewal(
    options: &RenewalOptions,
    stop_receiver: &mpsc::Receiver<()>,
) -> Result<bool, std::io::Error> {
    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(&options.renewal_script)
        .arg("-StepExecutable")
        .arg(&options.step_executable)
        .arg("-CaUrl")
        .arg(&options.ca_url)
        .arg("-RootCaCert")
        .arg(&options.root_ca_cert)
        .arg("-ServerCert")
        .arg(&options.server_cert)
        .arg("-ServerKey")
        .arg(&options.server_key)
        .arg("-ExpectedDnsName")
        .arg(&options.expected_dns_name)
        .arg("-BridgeServiceName")
        .arg(&options.bridge_service_name)
        .arg("-LogPath")
        .arg(&options.log_path)
        .spawn()?;
    wait_for_child(&mut child, stop_receiver)
}

fn wait_for_child(
    child: &mut Child,
    stop_receiver: &mpsc::Receiver<()>,
) -> Result<bool, std::io::Error> {
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(true);
            }
            return Err(std::io::Error::other(format!(
                "renewal script exited with {status}"
            )));
        }
        match stop_receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                child.kill()?;
                let _ = child.wait();
                return Ok(false);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

use std::{
    ffi::OsStr,
    fmt, io,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    os::windows::ffi::OsStrExt,
    thread,
    time::{Duration, Instant},
};

use super::{
    CreateSystemRestorePointResult, RestorePointDescription,
    path_policy::{ValidatedExecutable, validate_executable},
    protocol::{
        HelperErrorCode, PrivilegedOperation, PrivilegedResponse, RequestEnvelope,
        ResponseEnvelope, SecretToken, TOKEN_BYTES, generate_request_id, read_json_frame,
        tokens_match, write_json_frame,
    },
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    UI::{
        Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};
use zeroize::Zeroizing;

const SOCKET_TIMEOUT: Duration = Duration::from_secs(120);
const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(90);
const HELPER_FILENAME: &str = "supa-diska-klinah-privileged-helper.exe";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerError {
    AuthorizationCancelled,
    HelperUnavailable,
    Timeout,
    InvalidRequest,
    PrivilegeFailure,
    SystemRestoreFailure,
}

impl fmt::Display for BrokerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AuthorizationCancelled => "administrator authorization was cancelled or denied",
            Self::HelperUnavailable => "privileged helper is unavailable",
            Self::Timeout => "privileged helper timed out",
            Self::InvalidRequest => "privileged request was invalid or stale",
            Self::PrivilegeFailure => "privileged helper was not elevated",
            Self::SystemRestoreFailure => "Windows System Restore failed",
        })
    }
}

impl std::error::Error for BrokerError {}

impl From<io::Error> for BrokerError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Self::Timeout,
            _ => Self::HelperUnavailable,
        }
    }
}

impl From<HelperErrorCode> for BrokerError {
    fn from(code: HelperErrorCode) -> Self {
        match code {
            HelperErrorCode::InvalidRequest => Self::InvalidRequest,
            HelperErrorCode::PrivilegeFailure => Self::PrivilegeFailure,
            HelperErrorCode::SystemRestoreFailure => Self::SystemRestoreFailure,
        }
    }
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: hProcess is owned when SEE_MASK_NOCLOSEPROCESS succeeds.
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub fn create_system_restore_point(
    description: RestorePointDescription,
) -> Result<CreateSystemRestorePointResult, BrokerError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let token = SecretToken::generate()?;
    let request_id = generate_request_id()?;
    let request = RequestEnvelope::new(
        request_id,
        PrivilegedOperation::create_system_restore_point(description),
    )?;
    let helper = helper_path()?;
    let _process = launch_elevated(&helper, port, &token)?;
    exchange_once(
        listener,
        token.expose(),
        &request,
        Instant::now() + HANDSHAKE_DEADLINE,
    )
}

fn helper_path() -> Result<ValidatedExecutable, BrokerError> {
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or(BrokerError::HelperUnavailable)?;
    validate_executable(directory, &directory.join(HELPER_FILENAME))
        .map_err(|_| BrokerError::HelperUnavailable)
}

fn launch_elevated(
    helper: &ValidatedExecutable,
    port: u16,
    token: &SecretToken,
) -> Result<ProcessHandle, BrokerError> {
    let verb = wide("runas");
    let file = wide(helper.as_path().as_os_str());
    let parameters = launch_parameters(port, token);
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_NOCLOSEPROCESS,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: execute and its NUL-terminated strings remain alive for this synchronous call.
    if unsafe { ShellExecuteExW(&mut execute) } == 0 {
        let error = io::Error::last_os_error();
        return Err(map_launch_error(error));
    }
    if execute.hProcess.is_null() {
        Err(BrokerError::HelperUnavailable)
    } else {
        Ok(ProcessHandle(execute.hProcess))
    }
}

fn map_launch_error(error: io::Error) -> BrokerError {
    if matches!(error.raw_os_error(), Some(5 | 1223)) {
        BrokerError::AuthorizationCancelled
    } else {
        error.into()
    }
}

fn launch_parameters(port: u16, token: &SecretToken) -> Zeroizing<Vec<u16>> {
    let token_hex = token.to_hex();
    let parameters = Zeroizing::new(format!("--port {port} --token {}", token_hex.as_str()));
    Zeroizing::new(wide(parameters.as_str()))
}

fn exchange_once(
    listener: TcpListener,
    expected_token: &[u8; TOKEN_BYTES],
    request: &RequestEnvelope,
    deadline: Instant,
) -> Result<CreateSystemRestorePointResult, BrokerError> {
    listener.set_nonblocking(true)?;
    let (mut stream, peer) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return Err(BrokerError::Timeout);
            }
            Err(error) => return Err(error.into()),
        }
    };
    if !matches!(peer, SocketAddr::V4(address) if address.ip().is_loopback()) {
        return Err(BrokerError::HelperUnavailable);
    }
    configure_stream(&stream)?;
    let mut supplied_token = Zeroizing::new([0; TOKEN_BYTES]);
    use std::io::Read;
    stream.read_exact(supplied_token.as_mut())?;
    let authenticated = tokens_match(expected_token, &supplied_token);
    if !authenticated {
        return Err(BrokerError::HelperUnavailable);
    }

    write_json_frame(&mut stream, request)?;
    let response: ResponseEnvelope = read_json_frame(&mut stream)?;
    response
        .validate_for(&request.request_id)
        .map_err(|_| BrokerError::HelperUnavailable)?;
    match response.response {
        PrivilegedResponse::Success { result } => Ok(result),
        PrivilegedResponse::Error { code } => Err(code.into()),
    }
}

fn configure_stream(stream: &TcpStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_nodelay(true)
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

use std::mem::size_of;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::protocol::{PROTOCOL_VERSION, REQUEST_ID_BYTES, ResponseEnvelope};
    use std::io::{Read, Write};
    use zeroize::Zeroize;

    fn integration_exchange(
        client_token: [u8; TOKEN_BYTES],
    ) -> Result<CreateSystemRestorePointResult, BrokerError> {
        integration_exchange_with_delay(client_token, Duration::ZERO)
    }

    fn integration_exchange_with_delay(
        client_token: [u8; TOKEN_BYTES],
        response_delay: Duration,
    ) -> Result<CreateSystemRestorePointResult, BrokerError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let request = RequestEnvelope::new(
            [3; REQUEST_ID_BYTES],
            PrivilegedOperation::CreateSystemRestorePoint {
                description: "Before cleanup".into(),
            },
        )
        .unwrap();
        let request_id = request.request_id.clone();
        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            configure_stream(&stream).unwrap();
            stream.write_all(&client_token).unwrap();
            if client_token == [9; TOKEN_BYTES] {
                let _: RequestEnvelope = read_json_frame(&mut stream).unwrap();
                thread::sleep(response_delay);
                write_json_frame(
                    &mut stream,
                    &ResponseEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                        response: PrivilegedResponse::Success {
                            result: CreateSystemRestorePointResult {
                                sequence_number: 42,
                            },
                        },
                    },
                )
                .unwrap();
            } else {
                let mut closed = [0; 1];
                let _ = stream.read(&mut closed);
            }
        });
        let result = exchange_once(
            listener,
            &[9; TOKEN_BYTES],
            &request,
            Instant::now() + SOCKET_TIMEOUT,
        );
        client.join().unwrap();
        result
    }

    #[test]
    fn real_loopback_exchange_is_authenticated_and_one_shot() {
        assert_eq!(
            integration_exchange([9; TOKEN_BYTES])
                .unwrap()
                .sequence_number,
            42
        );
    }

    #[test]
    fn restore_point_response_can_arrive_after_five_seconds() {
        assert_eq!(
            integration_exchange_with_delay([9; TOKEN_BYTES], Duration::from_secs(6))
                .unwrap()
                .sequence_number,
            42
        );
    }

    #[test]
    fn wrong_token_is_rejected_before_the_request_is_sent() {
        assert!(matches!(
            integration_exchange([8; TOKEN_BYTES]),
            Err(BrokerError::HelperUnavailable)
        ));
    }

    #[test]
    fn broker_maps_failures_to_bounded_non_sensitive_codes() {
        assert_eq!(
            map_launch_error(io::Error::from_raw_os_error(1223)),
            BrokerError::AuthorizationCancelled
        );
        assert_eq!(
            BrokerError::from(io::Error::new(io::ErrorKind::TimedOut, "OS detail")),
            BrokerError::Timeout
        );
        assert_eq!(
            BrokerError::from(io::Error::other("OS detail")),
            BrokerError::HelperUnavailable
        );
        assert_eq!(
            BrokerError::from(HelperErrorCode::InvalidRequest),
            BrokerError::InvalidRequest
        );
        assert_eq!(
            BrokerError::from(HelperErrorCode::PrivilegeFailure),
            BrokerError::PrivilegeFailure
        );
        assert_eq!(
            BrokerError::from(HelperErrorCode::SystemRestoreFailure),
            BrokerError::SystemRestoreFailure
        );
    }

    #[test]
    fn launch_parameter_secret_owners_can_be_zeroized_without_logging_contents() {
        let token = SecretToken::parse(&"ab".repeat(TOKEN_BYTES)).unwrap();
        let mut parameters = launch_parameters(1234, &token);

        parameters.zeroize();

        assert!(parameters.is_empty() || parameters.iter().all(|unit| *unit == 0));
    }
}

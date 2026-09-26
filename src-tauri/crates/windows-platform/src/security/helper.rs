use std::{
    ffi::OsString,
    io::{self, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    time::Duration,
};

use super::{
    CreateSystemRestorePointResult, RestorePointDescription,
    protocol::{
        HelperErrorCode, PROTOCOL_VERSION, PrivilegedOperation, PrivilegedResponse,
        RequestEnvelope, ResponseEnvelope, SecretToken, read_json_frame, unix_time,
        write_json_frame,
    },
    restore_point::RestorePointBackend,
    system_changes::{SystemChangeBackend, apply_batch, validate_batch},
};
use crate::privilege::{PrivilegeProbe, ProcessPrivilege};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(90);
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HelperArguments {
    port: u16,
    token: SecretToken,
}

impl HelperArguments {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> io::Result<Self> {
        let args: Vec<_> = args.into_iter().collect();
        if args.len() != 4 || args[0] != "--port" || args[2] != "--token" {
            return Err(invalid_input("expected only --port and --token"));
        }
        let port = args[1]
            .to_str()
            .and_then(|value| value.parse().ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| invalid_input("invalid helper port"))?;
        let token = args[3]
            .to_str()
            .ok_or_else(|| invalid_input("invalid helper token"))
            .and_then(SecretToken::parse)?;
        Ok(Self { port, token })
    }
}

pub fn run(
    args: impl IntoIterator<Item = OsString>,
    backend: &impl RestorePointBackend,
    changes: &impl SystemChangeBackend,
) -> io::Result<()> {
    run_with(args, &ProcessPrivilege, backend, changes)
}

pub fn run_with(
    args: impl IntoIterator<Item = OsString>,
    privilege: &impl PrivilegeProbe,
    backend: &impl RestorePointBackend,
    changes: &impl SystemChangeBackend,
) -> io::Result<()> {
    let arguments = HelperArguments::parse(args)?;
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, arguments.port);
    let mut stream = TcpStream::connect_timeout(&address.into(), CONNECT_TIMEOUT)?;
    if !stream.peer_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "helper peer is not loopback",
        ));
    }
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_nodelay(true)?;
    stream.write_all(arguments.token.expose())?;

    let request: RequestEnvelope = match read_json_frame(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_json_frame(
                &mut stream,
                &error_response("", HelperErrorCode::InvalidRequest),
            );
            return Err(error);
        }
    };
    let response = dispatch(request, privilege, backend, changes);
    write_json_frame(&mut stream, &response)
}

pub fn dispatch(
    request: RequestEnvelope,
    privilege: &impl PrivilegeProbe,
    backend: &impl RestorePointBackend,
    changes: &impl SystemChangeBackend,
) -> ResponseEnvelope {
    let request_id = request.request_id.clone();
    let response = match unix_time()
        .and_then(|now| request.validate(now))
        .map_err(|_| HelperErrorCode::InvalidRequest)
        .and_then(|()| match privilege.is_elevated() {
            Ok(true) => Ok(()),
            _ => Err(HelperErrorCode::PrivilegeFailure),
        })
        .and_then(|()| match request.operation {
            PrivilegedOperation::CreateSystemRestorePoint { description } => {
                let description = RestorePointDescription::parse(description)
                    .map_err(|_| HelperErrorCode::InvalidRequest)?;
                backend
                    .create(&description)
                    .map(|sequence_number| PrivilegedResponse::Success {
                        result: CreateSystemRestorePointResult { sequence_number },
                    })
                    .map_err(map_restore_point_error)
            }
            PrivilegedOperation::ApplySystemChanges { changes: items } => {
                if !validate_batch(&items) {
                    return Err(HelperErrorCode::InvalidRequest);
                }
                Ok(PrivilegedResponse::SystemChangesApplied {
                    results: apply_batch(&items, changes),
                })
            }
        }) {
        Ok(response) => response,
        Err(code) => PrivilegedResponse::Error { code },
    };
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        response,
    }
}

fn map_restore_point_error(_: super::restore_point::RestorePointError) -> HelperErrorCode {
    HelperErrorCode::SystemRestoreFailure
}

fn error_response(request_id: &str, code: HelperErrorCode) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.to_owned(),
        response: PrivilegedResponse::Error { code },
    }
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        privilege::PrivilegeProbe,
        security::{
            protocol::{AUTHORIZATION_LIFETIME_SECONDS, REQUEST_ID_BYTES},
            restore_point::RestorePointError,
        },
    };
    use crate::{
        security::system_changes::{HelperChange, HelperChangeItem},
        system_change::AdapterError,
    };
    use cleanup_core::system_change::{ChangeOutcome, PriorState};
    use std::{cell::Cell, io};

    struct CountingChanges(Cell<u32>);

    impl SystemChangeBackend for CountingChanges {
        fn observe(&self, _change: &HelperChange) -> Result<PriorState, AdapterError> {
            Ok(PriorState::Enabled { enabled: true })
        }

        fn apply(&self, _change: &HelperChange) -> Result<(), AdapterError> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }

    fn changes() -> CountingChanges {
        CountingChanges(Cell::new(0))
    }

    fn batch(items: Vec<HelperChangeItem>) -> RequestEnvelope {
        RequestEnvelope::new(
            [5; REQUEST_ID_BYTES],
            PrivilegedOperation::ApplySystemChanges { changes: items },
        )
        .unwrap()
    }

    fn hibernate_off() -> HelperChangeItem {
        HelperChangeItem {
            change: HelperChange::SetHibernation { enabled: false },
            expected_prior: PriorState::Enabled { enabled: true },
        }
    }

    struct FixedPrivilege(bool);

    impl PrivilegeProbe for FixedPrivilege {
        fn is_elevated(&self) -> io::Result<bool> {
            Ok(self.0)
        }
    }

    struct CountingBackend(Cell<u32>);

    impl RestorePointBackend for CountingBackend {
        fn create(&self, _description: &RestorePointDescription) -> Result<i64, RestorePointError> {
            self.0.set(self.0.get() + 1);
            Ok(77)
        }
    }

    fn request() -> RequestEnvelope {
        RequestEnvelope::new(
            [5; REQUEST_ID_BYTES],
            PrivilegedOperation::CreateSystemRestorePoint {
                description: "Before cleanup".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn parser_rejects_missing_extra_and_malformed_arguments() {
        for args in [
            vec!["--port", "42"],
            vec!["--port", "42", "--token", "00", "extra"],
            vec!["--port", "0", "--token", &"00".repeat(32)],
        ] {
            assert!(HelperArguments::parse(args.into_iter().map(OsString::from)).is_err());
        }
    }

    #[test]
    fn unelevated_dispatch_is_rejected_before_backend_access() {
        let backend = CountingBackend(Cell::new(0));
        let response = dispatch(request(), &FixedPrivilege(false), &backend, &changes());
        assert_eq!(backend.0.get(), 0);
        assert!(matches!(
            response.response,
            PrivilegedResponse::Error {
                code: HelperErrorCode::PrivilegeFailure
            }
        ));
        let system = changes();
        let response = dispatch(
            batch(vec![hibernate_off()]),
            &FixedPrivilege(false),
            &backend,
            &system,
        );
        assert_eq!(system.0.get(), 0);
        assert!(matches!(
            response.response,
            PrivilegedResponse::Error {
                code: HelperErrorCode::PrivilegeFailure
            }
        ));
    }

    #[test]
    fn elevated_batch_returns_one_result_per_change() {
        let system = changes();
        let mut already = hibernate_off();
        already.change = HelperChange::SetHibernation { enabled: true };
        let response = dispatch(
            batch(vec![hibernate_off(), already]),
            &FixedPrivilege(true),
            &CountingBackend(Cell::new(0)),
            &system,
        );
        let PrivilegedResponse::SystemChangesApplied { results } = response.response else {
            panic!("expected batch results");
        };
        assert_eq!(
            results
                .iter()
                .map(|result| result.outcome)
                .collect::<Vec<_>>(),
            [ChangeOutcome::Applied, ChangeOutcome::AlreadyApplied]
        );
        assert_eq!(system.0.get(), 1);
    }

    #[test]
    fn empty_or_oversized_batches_are_rejected_before_system_access() {
        for items in [vec![], vec![hibernate_off(); 33]] {
            let system = changes();
            let response = dispatch(
                batch(items),
                &FixedPrivilege(true),
                &CountingBackend(Cell::new(0)),
                &system,
            );
            assert_eq!(system.0.get(), 0);
            assert!(matches!(
                response.response,
                PrivilegedResponse::Error {
                    code: HelperErrorCode::InvalidRequest
                }
            ));
        }
    }

    #[test]
    fn restore_point_backend_failures_use_one_non_sensitive_code() {
        for error in [
            RestorePointError::Windows(io::Error::other("sensitive OS detail")),
            RestorePointError::Com(-1),
            RestorePointError::Status(5),
            RestorePointError::MissingEntryPoint,
            RestorePointError::NotCreated,
        ] {
            assert_eq!(
                map_restore_point_error(error),
                HelperErrorCode::SystemRestoreFailure
            );
        }
    }

    #[test]
    fn elevated_dispatch_executes_exactly_one_allowlisted_operation() {
        let backend = CountingBackend(Cell::new(0));
        let response = dispatch(request(), &FixedPrivilege(true), &backend, &changes());
        assert_eq!(backend.0.get(), 1);
        assert!(matches!(
            response.response,
            PrivilegedResponse::Success { .. }
        ));
    }

    #[test]
    fn stale_dispatch_is_rejected_before_backend_access() {
        let backend = CountingBackend(Cell::new(0));
        let mut stale = request();
        stale.issued_at = 1;
        stale.expires_at = 1 + AUTHORIZATION_LIFETIME_SECONDS;
        let response = dispatch(stale, &FixedPrivilege(true), &backend, &changes());
        assert_eq!(backend.0.get(), 0);
        assert!(matches!(
            response.response,
            PrivilegedResponse::Error {
                code: HelperErrorCode::InvalidRequest
            }
        ));
    }
}

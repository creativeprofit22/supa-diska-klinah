use std::{
    io::{self, Read, Write},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

use super::{CreateSystemRestorePointResult, RestorePointDescription};

pub const PROTOCOL_VERSION: u8 = 1;
pub const TOKEN_BYTES: usize = 32;
pub const REQUEST_ID_BYTES: usize = 16;
pub const MAX_FRAME_BYTES: usize = 4 * 1024;
pub const AUTHORIZATION_LIFETIME_SECONDS: u64 = 60;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum PrivilegedOperation {
    CreateSystemRestorePoint { description: String },
}

impl PrivilegedOperation {
    pub fn create_system_restore_point(description: RestorePointDescription) -> Self {
        Self::CreateSystemRestorePoint {
            description: description.as_str().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestEnvelope {
    pub protocol_version: u8,
    pub request_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub operation: PrivilegedOperation,
}

impl RequestEnvelope {
    pub fn new(
        request_id: [u8; REQUEST_ID_BYTES],
        operation: PrivilegedOperation,
    ) -> io::Result<Self> {
        let issued_at = unix_time()?;
        Ok(Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: encode_hex(&request_id),
            issued_at,
            expires_at: issued_at + AUTHORIZATION_LIFETIME_SECONDS,
            operation,
        })
    }

    pub fn validate(&self, now: u64) -> io::Result<()> {
        if self.protocol_version != PROTOCOL_VERSION
            || decode_hex::<REQUEST_ID_BYTES>(&self.request_id).is_none()
            || self.issued_at > now
            || self.expires_at < now
            || self.expires_at < self.issued_at
            || self.expires_at - self.issued_at > AUTHORIZATION_LIFETIME_SECONDS
        {
            return Err(invalid_data("invalid or stale privileged request"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
pub enum PrivilegedResponse {
    Success {
        result: CreateSystemRestorePointResult,
    },
    Error {
        code: HelperErrorCode,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HelperErrorCode {
    InvalidRequest,
    NotElevated,
    OperationFailed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResponseEnvelope {
    pub protocol_version: u8,
    pub request_id: String,
    pub response: PrivilegedResponse,
}

impl ResponseEnvelope {
    pub fn validate_for(&self, request_id: &str) -> io::Result<()> {
        if self.protocol_version != PROTOCOL_VERSION || self.request_id != request_id {
            Err(invalid_data("invalid privileged response"))
        } else {
            Ok(())
        }
    }
}

pub struct SecretToken(Zeroizing<[u8; TOKEN_BYTES]>);

impl SecretToken {
    pub fn generate() -> io::Result<Self> {
        let mut bytes = Zeroizing::new([0; TOKEN_BYTES]);
        getrandom::fill(bytes.as_mut()).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(Self(bytes))
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        decode_hex(value)
            .map(Self)
            .ok_or_else(|| invalid_data("invalid helper token"))
    }

    pub fn expose(&self) -> &[u8; TOKEN_BYTES] {
        &self.0
    }

    pub fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(encode_hex(&self.0[..]))
    }
}

pub fn generate_request_id() -> io::Result<[u8; REQUEST_ID_BYTES]> {
    let mut request_id = [0; REQUEST_ID_BYTES];
    getrandom::fill(&mut request_id).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(request_id)
}

pub fn tokens_match(expected: &[u8; TOKEN_BYTES], supplied: &[u8; TOKEN_BYTES]) -> bool {
    expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

pub fn write_json_frame(mut writer: impl Write, value: &impl Serialize) -> io::Result<()> {
    let frame = serde_json::to_vec(value).map_err(invalid_json)?;
    if frame.len() > MAX_FRAME_BYTES {
        return Err(invalid_data("privileged frame exceeds 4 KiB"));
    }
    let mut message = Vec::with_capacity(4 + frame.len());
    message.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    message.extend_from_slice(&frame);
    writer.write_all(&message)
}

pub fn read_json_frame<T: DeserializeOwned>(mut reader: impl Read) -> io::Result<T> {
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MAX_FRAME_BYTES {
        return Err(invalid_data("invalid privileged frame size"));
    }
    let mut frame = vec![0; size];
    reader.read_exact(&mut frame)?;
    serde_json::from_slice(&frame).map_err(invalid_json)
}

pub fn unix_time() -> io::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(io::Error::other)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex<const N: usize>(value: &str) -> Option<Zeroizing<[u8; N]>> {
    if value.len() != N * 2 {
        return None;
    }
    let mut bytes = Zeroizing::new([0; N]);
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(bytes)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn invalid_json(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zeroize::Zeroize;

    fn request() -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "11".repeat(REQUEST_ID_BYTES),
            issued_at: 100,
            expires_at: 160,
            operation: PrivilegedOperation::CreateSystemRestorePoint {
                description: "Before cleanup".into(),
            },
        }
    }

    #[test]
    fn framing_round_trips_a_request() {
        let mut bytes = Vec::new();
        write_json_frame(&mut bytes, &request()).unwrap();
        assert_eq!(
            read_json_frame::<RequestEnvelope>(Cursor::new(bytes)).unwrap(),
            request()
        );
    }

    #[test]
    fn framing_round_trips_a_response() {
        let response = ResponseEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: "11".repeat(REQUEST_ID_BYTES),
            response: PrivilegedResponse::Success {
                result: CreateSystemRestorePointResult {
                    sequence_number: 42,
                },
            },
        };
        let mut bytes = Vec::new();
        write_json_frame(&mut bytes, &response).unwrap();
        assert_eq!(
            read_json_frame::<ResponseEnvelope>(Cursor::new(bytes)).unwrap(),
            response
        );
    }

    #[test]
    fn framing_rejects_oversized_truncated_and_malformed_json() {
        assert!(read_json_frame::<RequestEnvelope>(Cursor::new((4097_u32).to_be_bytes())).is_err());
        assert!(read_json_frame::<RequestEnvelope>(Cursor::new([0, 0, 0, 5, b'{'])).is_err());
        assert!(read_json_frame::<RequestEnvelope>(Cursor::new([0, 0, 0, 1, b'{'])).is_err());
    }

    #[test]
    fn parser_rejects_unknown_operations_and_fields() {
        for json in [
            br#"{"protocolVersion":1,"requestId":"11111111111111111111111111111111","issuedAt":100,"expiresAt":160,"operation":{"operation":"deletePath","path":"C:\\"}}"#.as_slice(),
            br#"{"protocolVersion":1,"requestId":"11111111111111111111111111111111","issuedAt":100,"expiresAt":160,"extra":true,"operation":{"operation":"createSystemRestorePoint","description":"ok"}}"#.as_slice(),
        ] {
            let mut frame = (json.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(json);
            assert!(read_json_frame::<RequestEnvelope>(Cursor::new(frame)).is_err());
        }
    }

    #[test]
    fn freshness_rejects_expired_future_and_overlong_requests() {
        let mut value = request();
        assert!(value.validate(120).is_ok());
        assert!(value.validate(161).is_err());
        value.issued_at = 121;
        assert!(value.validate(120).is_err());
        value.issued_at = 100;
        value.expires_at = 161;
        assert!(value.validate(120).is_err());
    }

    #[test]
    fn token_comparison_rejects_wrong_tokens() {
        let expected = [7; TOKEN_BYTES];
        let mut supplied = expected;
        assert!(tokens_match(&expected, &supplied));
        supplied[TOKEN_BYTES - 1] ^= 1;
        assert!(!tokens_match(&expected, &supplied));
    }

    #[test]
    fn secret_token_owners_can_be_zeroized_without_exposing_contents() {
        let mut token = SecretToken::parse(&"ab".repeat(TOKEN_BYTES)).unwrap();
        let mut hex = token.to_hex();

        token.0.zeroize();
        hex.zeroize();

        assert!(token.expose().iter().all(|byte| *byte == 0));
        assert!(hex.is_empty() || hex.as_bytes().iter().all(|byte| *byte == 0));
    }
}

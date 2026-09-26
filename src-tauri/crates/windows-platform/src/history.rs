//! Bounded, stateless history paging contracts. Cursors are read boundaries, not authority.
use serde::{Deserialize, Serialize};

pub const MAX_CURSOR_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryKind {
    Cleanup,
    Vendor,
}

impl HistoryKind {
    pub fn default_limit(self) -> usize {
        match self {
            Self::Cleanup => 20,
            Self::Vendor => 64,
        }
    }

    pub fn max_limit(self) -> usize {
        match self {
            Self::Cleanup => 100,
            Self::Vendor => 64,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidHistoryRequest;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoryRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage<T> {
    pub records: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCursor {
    version: u8,
    kind: HistoryKind,
    pub timestamp: u64,
    pub id: String,
}

impl HistoryCursor {
    pub fn decode(input: &str, kind: HistoryKind) -> Result<Self, InvalidHistoryRequest> {
        if input.len() > MAX_CURSOR_BYTES {
            return Err(InvalidHistoryRequest);
        }
        let cursor: Self = serde_json::from_str(input).map_err(|_| InvalidHistoryRequest)?;
        if cursor.version != 1
            || cursor.kind != kind
            || cursor.id.len() != 32
            || !cursor.id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(InvalidHistoryRequest);
        }
        Ok(cursor)
    }

    pub fn encode(
        kind: HistoryKind,
        timestamp: u64,
        id: &str,
    ) -> Result<String, InvalidHistoryRequest> {
        let cursor = Self {
            version: 1,
            kind,
            timestamp,
            id: id.to_owned(),
        };
        let encoded = serde_json::to_string(&cursor).map_err(|_| InvalidHistoryRequest)?;
        Self::decode(&encoded, kind)?;
        Ok(encoded)
    }
}

impl HistoryRequest {
    pub fn validate(
        &self,
        kind: HistoryKind,
    ) -> Result<(Option<HistoryCursor>, usize), InvalidHistoryRequest> {
        let limit = self.limit.unwrap_or_else(|| kind.default_limit());
        if limit == 0 || limit > kind.max_limit() {
            return Err(InvalidHistoryRequest);
        }
        let cursor = self
            .cursor
            .as_deref()
            .map(|value| HistoryCursor::decode(value, kind))
            .transpose()?;
        Ok((cursor, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_strict_versioned_kind_bound_and_bounded() {
        let encoded = HistoryCursor::encode(
            HistoryKind::Cleanup,
            u64::MAX,
            "0123456789abcdef0123456789ABCDEF",
        )
        .unwrap();
        let cursor = HistoryCursor::decode(&encoded, HistoryKind::Cleanup).unwrap();
        assert_eq!(cursor.timestamp, u64::MAX);
        assert_eq!(cursor.id, "0123456789abcdef0123456789ABCDEF");
        assert!(HistoryCursor::decode(&encoded, HistoryKind::Vendor).is_err());
        for input in [
            "null",
            "{}",
            "not json",
            r#"{"version":2,"kind":"cleanup","timestamp":0,"id":"0123456789abcdef0123456789abcdef"}"#,
            r#"{"version":1,"kind":"other","timestamp":0,"id":"0123456789abcdef0123456789abcdef"}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":-1,"id":"0123456789abcdef0123456789abcdef"}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":1.5,"id":"0123456789abcdef0123456789abcdef"}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":"1","id":"0123456789abcdef0123456789abcdef"}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":0,"id":""}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":0,"id":"0123456789abcdef0123456789abcdef","extra":0}"#,
            r#"{"version":1,"kind":"cleanup","timestamp":0,"id":"0123456789abcdef0123456789abcdef","id":"abcdef0123456789abcdef0123456789"}"#,
        ] {
            assert!(
                HistoryCursor::decode(input, HistoryKind::Cleanup).is_err(),
                "{input}"
            );
        }
        for kind in [HistoryKind::Cleanup, HistoryKind::Vendor] {
            for id in [
                "a",
                "../bad",
                "0123456789abcdef0123456789abcdeg",
                "0123456789abcdef0123456789abcdef00",
            ] {
                let input =
                    serde_json::json!({"version": 1, "kind": kind, "timestamp": 0, "id": id})
                        .to_string();
                assert!(HistoryCursor::decode(&input, kind).is_err());
                assert!(
                    HistoryRequest {
                        cursor: Some(input),
                        limit: None
                    }
                    .validate(kind)
                    .is_err()
                );
                assert!(HistoryCursor::encode(kind, 0, id).is_err());
            }
        }
        let boundary = format!("{encoded}{}", " ".repeat(MAX_CURSOR_BYTES - encoded.len()));
        assert!(HistoryCursor::decode(&boundary, HistoryKind::Cleanup).is_ok());
        assert!(HistoryCursor::decode(&(boundary + " "), HistoryKind::Cleanup).is_err());
        assert!(HistoryCursor::encode(HistoryKind::Cleanup, 0, &"é".repeat(128)).is_err());
    }

    #[test]
    fn request_limits_and_unknown_fields_are_rejected() {
        for (kind, default, max) in [
            (HistoryKind::Cleanup, 20, 100),
            (HistoryKind::Vendor, 64, 64),
        ] {
            assert_eq!(HistoryRequest::default().validate(kind).unwrap().1, default);
            for limit in [0, max + 1, usize::MAX] {
                assert!(
                    HistoryRequest {
                        cursor: None,
                        limit: Some(limit)
                    }
                    .validate(kind)
                    .is_err()
                );
            }
            assert!(
                HistoryRequest {
                    cursor: None,
                    limit: Some(max)
                }
                .validate(kind)
                .is_ok()
            );
        }
        assert!(serde_json::from_str::<HistoryRequest>(r#"{"offset":0}"#).is_err());
        assert!(serde_json::from_str::<HistoryRequest>(r#"{"cursor":{}}"#).is_err());
        assert!(serde_json::from_str::<HistoryRequest>(r#"{"limit":-1}"#).is_err());
    }
}

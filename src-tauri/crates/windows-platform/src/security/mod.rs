pub mod broker;
pub mod helper;
pub mod path_policy;
pub mod protocol;
pub mod restore_point;

use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_RESTORE_POINT_DESCRIPTION_UTF16: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestorePointDescription(String);

impl RestorePointDescription {
    pub fn parse(value: String) -> Result<Self, DescriptionError> {
        if value.is_empty() {
            return Err(DescriptionError::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(DescriptionError::ControlCharacter);
        }
        if value.encode_utf16().count() > MAX_RESTORE_POINT_DESCRIPTION_UTF16 {
            return Err(DescriptionError::TooLong);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptionError {
    Empty,
    ControlCharacter,
    TooLong,
}

impl fmt::Display for DescriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "description must not be empty",
            Self::ControlCharacter => "description must not contain control characters",
            Self::TooLong => "description exceeds 128 UTF-16 code units",
        })
    }
}

impl std::error::Error for DescriptionError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSystemRestorePointResult {
    pub sequence_number: i64,
}

#[cfg(test)]
mod tests {
    use super::{DescriptionError, RestorePointDescription};

    #[test]
    fn description_accepts_128_utf16_units() {
        let value = "a".repeat(126) + "😀";
        assert!(RestorePointDescription::parse(value).is_ok());
    }

    #[test]
    fn description_rejects_empty_control_and_oversized_input() {
        assert_eq!(
            RestorePointDescription::parse(String::new()).unwrap_err(),
            DescriptionError::Empty
        );
        assert_eq!(
            RestorePointDescription::parse("line\nbreak".into()).unwrap_err(),
            DescriptionError::ControlCharacter
        );
        assert_eq!(
            RestorePointDescription::parse("😀".repeat(65)).unwrap_err(),
            DescriptionError::TooLong
        );
    }
}

//! Unprivileged side of the helper exchange for system-change plans.

use cleanup_core::system_change::{FailureCode, PriorState, SystemChange};

use super::{HelperClient, HelperClientError, HelperItemResult};
use crate::security::{
    broker::{self, BrokerError},
    system_changes::{HelperChange, HelperChangeItem},
};

pub struct BrokerHelperClient;

impl HelperClient for BrokerHelperClient {
    fn apply(
        &self,
        items: &[(SystemChange, PriorState)],
    ) -> Result<Vec<HelperItemResult>, HelperClientError> {
        let items = to_helper_items(items)?;
        broker::apply_system_changes(items)
            .map(|results| {
                results
                    .into_iter()
                    .map(|result| HelperItemResult {
                        prior: result.prior,
                        outcome: result.outcome,
                    })
                    .collect()
            })
            .map_err(map_broker_error)
    }
}

pub(crate) fn to_helper_items(
    items: &[(SystemChange, PriorState)],
) -> Result<Vec<HelperChangeItem>, HelperClientError> {
    items
        .iter()
        .map(|(change, expected_prior)| {
            HelperChange::from_system_change(change)
                .map(|change| HelperChangeItem {
                    change,
                    expected_prior: expected_prior.clone(),
                })
                .ok_or(HelperClientError::NotStarted(FailureCode::InvalidRequest))
        })
        .collect()
}

pub(crate) fn map_broker_error(error: BrokerError) -> HelperClientError {
    match error {
        BrokerError::AuthorizationCancelled | BrokerError::PrivilegeFailure => {
            HelperClientError::Denied
        }
        BrokerError::HelperUnavailable => {
            HelperClientError::NotStarted(FailureCode::HelperUnavailable)
        }
        BrokerError::Timeout => HelperClientError::NotStarted(FailureCode::Timeout),
        BrokerError::InvalidRequest | BrokerError::SystemRestoreFailure => {
            HelperClientError::NotStarted(FailureCode::InvalidRequest)
        }
        BrokerError::ResponseLost => HelperClientError::Indeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cleanup_core::system_change::{
        CatalogId, EntryName, StartupEntryRef, StartupLocation, StartupScope,
    };

    #[test]
    fn denial_and_lost_responses_map_to_distinct_outcomes() {
        assert_eq!(
            map_broker_error(BrokerError::AuthorizationCancelled),
            HelperClientError::Denied
        );
        assert_eq!(
            map_broker_error(BrokerError::PrivilegeFailure),
            HelperClientError::Denied
        );
        assert_eq!(
            map_broker_error(BrokerError::ResponseLost),
            HelperClientError::Indeterminate
        );
        assert_eq!(
            map_broker_error(BrokerError::HelperUnavailable),
            HelperClientError::NotStarted(FailureCode::HelperUnavailable)
        );
    }

    #[test]
    fn standard_changes_are_refused_before_the_helper() {
        let user = SystemChange::SetStartupEntry {
            entry: StartupEntryRef {
                scope: StartupScope::User,
                location: StartupLocation::Run,
                name: EntryName::parse("App").unwrap(),
            },
            enabled: false,
        };
        assert_eq!(
            to_helper_items(&[(user, PriorState::Enabled { enabled: true })]).unwrap_err(),
            HelperClientError::NotStarted(FailureCode::InvalidRequest)
        );
        let service = SystemChange::SetServiceStartType {
            catalog_id: CatalogId::parse("diagtrack").unwrap(),
            start_type: cleanup_core::system_change::ServiceStartType::Disabled,
        };
        assert_eq!(
            to_helper_items(&[(service, PriorState::NotApplicable)])
                .unwrap()
                .len(),
            1
        );
    }
}

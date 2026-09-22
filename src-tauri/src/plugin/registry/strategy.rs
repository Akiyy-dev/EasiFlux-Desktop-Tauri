use super::*;
use crate::plugin::{
    contribution::{PluginCommandActionId, PluginCommandParams},
    record::PluginIdentity,
    strategy::{error, PluginStrategyParams},
    workflow::AuthorityRequest,
};

#[derive(Clone)]
pub(crate) struct CapturedStrategy {
    pub(crate) params: PluginStrategyParams,
    pub(crate) identity: PluginIdentity,
    pub(crate) requested: Vec<String>,
}
impl PluginRegistry {
    pub(crate) fn capture_strategy(
        &self,
        request: &AuthorityRequest,
    ) -> AppResult<CapturedStrategy> {
        request
            .validate()
            .map_err(|_| error("plugin_strategy_invalid_request"))?;
        let publication = &self.publication;
        let Runtime::Available { state, .. } = &publication.runtime else {
            return Err(error("plugin_strategy_unavailable"));
        };
        if self.removal_authority_unreconciled
            || self.ownership_requires_retry()
            || publication.local_summary.status == LocalDiscoveryStatus::Unavailable
            || publication.ownership_summary.status != LocalDiscoveryStatus::Available
        {
            return Err(error("plugin_strategy_unavailable"));
        }
        if request.expected_catalog_generation != publication.catalog_generation.to_string()
            || request.expected_revision != state.revision.to_string()
        {
            return Err(error("plugin_strategy_stale"));
        }
        let id = PluginId::parse(&request.plugin_id)
            .map_err(|_| error("plugin_strategy_invalid_request"))?;
        let record = publication
            .locals
            .get(&id)
            .ok_or_else(|| error("plugin_strategy_stale"))?;
        if !matches!(
            Self::management_for_record(publication, record),
            PluginManagement::External | PluginManagement::Managed
        ) || !is_enabled(state, record)
        {
            return Err(error("plugin_strategy_stale"));
        }
        let contribution = record
            .manifest()
            .contributions
            .iter()
            .find(|c| c.contribution_id.as_str() == request.contribution_id)
            .ok_or_else(|| error("plugin_strategy_stale"))?;
        let PluginCommandParams::Strategy(params) = &contribution.params else {
            return Err(error("plugin_strategy_denied"));
        };
        if record.manifest().schema_version != 6
            || contribution.action_id != PluginCommandActionId::SandboxStrategy
        {
            return Err(error("plugin_strategy_denied"));
        }
        Ok(CapturedStrategy {
            params: params.clone(),
            identity: record.identity(),
            requested: record.manifest().requested_capabilities.clone(),
        })
    }
}

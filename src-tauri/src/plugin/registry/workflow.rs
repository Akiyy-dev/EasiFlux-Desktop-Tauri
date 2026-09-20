use super::*;
use crate::plugin::{
    contribution::{PluginCommandActionId, PluginCommandParams},
    record::PluginIdentity,
    workflow::{error, AuthorityRequest, PluginWorkflowParams},
};

#[derive(Clone)]
pub(crate) struct CapturedWorkflow {
    pub(crate) params: PluginWorkflowParams,
    pub(crate) identity: PluginIdentity,
    pub(crate) requested: Vec<String>,
}
impl PluginRegistry {
    pub(crate) fn capture_workflow(
        &self,
        request: &AuthorityRequest,
    ) -> AppResult<CapturedWorkflow> {
        request.validate()?;
        let publication = &self.publication;
        let Runtime::Available { state, .. } = &publication.runtime else {
            return Err(error("plugin_workflow_unavailable"));
        };
        if self.removal_authority_unreconciled
            || self.ownership_requires_retry()
            || publication.local_summary.status == LocalDiscoveryStatus::Unavailable
            || publication.ownership_summary.status != LocalDiscoveryStatus::Available
        {
            return Err(error("plugin_workflow_unavailable"));
        }
        if request.expected_catalog_generation != publication.catalog_generation.to_string()
            || request.expected_revision != state.revision.to_string()
        {
            return Err(error("plugin_workflow_stale"));
        }
        let id = PluginId::parse(&request.plugin_id)
            .map_err(|_| error("plugin_workflow_invalid_request"))?;
        let record = publication
            .locals
            .get(&id)
            .or_else(|| self.builtins.get(&id))
            .ok_or_else(|| error("plugin_workflow_not_found"))?;
        if !matches!(
            Self::management_for_record(publication, record),
            PluginManagement::External | PluginManagement::Managed | PluginManagement::BuiltIn
        ) {
            return Err(error("plugin_workflow_unavailable"));
        }
        if !is_enabled(state, record) {
            return Err(error("plugin_workflow_disabled"));
        }
        let contribution = record
            .manifest()
            .contributions
            .iter()
            .find(|c| c.contribution_id.as_str() == request.contribution_id)
            .ok_or_else(|| error("plugin_workflow_not_found"))?;
        let PluginCommandParams::AccountWorkflow(params) = &contribution.params else {
            return Err(error("plugin_workflow_not_supported"));
        };
        if record.manifest().schema_version != 5
            || contribution.action_id != PluginCommandActionId::SandboxAccountWorkflow
        {
            return Err(error("plugin_workflow_not_supported"));
        }
        Ok(CapturedWorkflow {
            params: params.clone(),
            identity: record.identity(),
            requested: record.manifest().requested_capabilities.clone(),
        })
    }
}

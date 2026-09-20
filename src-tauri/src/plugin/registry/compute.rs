use super::*;
use crate::plugin::compute::{error, ComputeRequest, PluginComputeParams};
use crate::plugin::contribution::{PluginCommandActionId, PluginCommandParams};
use crate::plugin::record::PluginIdentity;

pub(crate) struct CapturedCompute {
    pub(crate) params: PluginComputeParams,
    identity: PluginIdentity,
}

impl PluginRegistry {
    /// Resolves only published, reconciled, content-bound enabled authority.
    /// Never retries I/O or accepts caller-supplied guest bytes.
    pub(crate) fn capture_compute(&self, request: &ComputeRequest) -> AppResult<CapturedCompute> {
        request.validate()?;
        let publication = &self.publication;
        let Runtime::Available { state, .. } = &publication.runtime else {
            return Err(error("plugin_compute_unavailable"));
        };
        if self.removal_authority_unreconciled
            || self.ownership_requires_retry()
            || publication.local_summary.status == LocalDiscoveryStatus::Unavailable
            || publication.ownership_summary.status != LocalDiscoveryStatus::Available
        {
            return Err(error("plugin_compute_unavailable"));
        }
        if request.expected_catalog_generation != publication.catalog_generation.to_string()
            || request.expected_revision != state.revision.to_string()
        {
            return Err(error("plugin_compute_stale"));
        }
        let id = PluginId::parse(&request.plugin_id)
            .map_err(|_| error("plugin_compute_invalid_request"))?;
        let record = publication
            .locals
            .get(&id)
            .or_else(|| self.builtins.get(&id))
            .ok_or_else(|| error("plugin_compute_not_found"))?;
        if !matches!(
            Self::management_for_record(publication, record),
            PluginManagement::External | PluginManagement::Managed | PluginManagement::BuiltIn
        ) {
            return Err(error("plugin_compute_unavailable"));
        }
        if !is_enabled(state, record) {
            return Err(error("plugin_compute_disabled"));
        }
        let contribution = record
            .manifest()
            .contributions
            .iter()
            .find(|c| c.contribution_id.as_str() == request.contribution_id)
            .ok_or_else(|| error("plugin_compute_not_found"))?;
        let PluginCommandParams::ComputeSeries(params) = &contribution.params else {
            return Err(error("plugin_compute_not_supported"));
        };
        if !(4..=5).contains(&record.manifest().schema_version)
            || contribution.action_id != PluginCommandActionId::SandboxComputeSeries
        {
            return Err(error("plugin_compute_not_supported"));
        }
        Ok(CapturedCompute {
            params: params.clone(),
            identity: record.identity(),
        })
    }

    pub(crate) fn revalidate_compute(
        &self,
        request: &ComputeRequest,
        captured: &CapturedCompute,
    ) -> AppResult<()> {
        let current = self.capture_compute(request)?;
        if current.identity != captured.identity || current.params != captured.params {
            return Err(error("plugin_compute_stale"));
        }
        Ok(())
    }
}

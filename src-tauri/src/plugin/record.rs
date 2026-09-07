use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::plugin::manifest::{
    PluginId, PluginManifestV1, PluginPublisherId, PluginSource, APPROVAL_FINGERPRINT_NONE,
};

#[allow(dead_code)]
const LOCAL_FINGERPRINT_DOMAIN: &[u8] = b"EasiFlux.localDeclarative.manifest.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginRecord {
    manifest: PluginManifestV1,
    source: PluginSource,
    approval_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginIdentity {
    pub(crate) id: PluginId,
    pub(crate) source: PluginSource,
    pub(crate) publisher_id: PluginPublisherId,
    pub(crate) approval_fingerprint: String,
}

#[allow(dead_code)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalManifestV1<'a> {
    schema_version: u32,
    id: &'a PluginId,
    publisher_id: &'a PluginPublisherId,
    publisher: &'a str,
    name: &'a str,
    description: &'a str,
    version: &'a Version,
    contributions: &'a [serde_json::Value],
    requested_capabilities: &'a [String],
}

impl<'a> From<&'a PluginManifestV1> for CanonicalManifestV1<'a> {
    fn from(manifest: &'a PluginManifestV1) -> Self {
        Self {
            schema_version: manifest.schema_version,
            id: &manifest.id,
            publisher_id: &manifest.publisher_id,
            publisher: &manifest.publisher,
            name: &manifest.name,
            description: &manifest.description,
            version: &manifest.version,
            contributions: &manifest.contributions,
            requested_capabilities: &manifest.requested_capabilities,
        }
    }
}

impl PluginRecord {
    pub(crate) fn built_in(manifest: PluginManifestV1) -> Result<Self, String> {
        manifest.validate()?;
        Ok(Self {
            manifest,
            source: PluginSource::BuiltIn,
            approval_fingerprint: APPROVAL_FINGERPRINT_NONE.to_owned(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn local_declarative(manifest: PluginManifestV1) -> Result<Self, String> {
        manifest.validate()?;
        let canonical = CanonicalManifestV1::from(&manifest);
        let bytes =
            serde_json::to_vec(&canonical).map_err(|_| "manifest fingerprint failed".to_owned())?;
        let mut digest = Sha256::new();
        digest.update(LOCAL_FINGERPRINT_DOMAIN);
        digest.update(bytes);
        Ok(Self {
            manifest,
            source: PluginSource::LocalDeclarative,
            approval_fingerprint: format!("v1:sha256:{}", hex::encode(digest.finalize())),
        })
    }

    pub(crate) fn manifest(&self) -> &PluginManifestV1 {
        &self.manifest
    }

    pub(crate) fn source(&self) -> PluginSource {
        self.source
    }

    #[allow(dead_code)]
    pub(crate) fn approval_fingerprint(&self) -> &str {
        &self.approval_fingerprint
    }

    pub(crate) fn identity(&self) -> PluginIdentity {
        PluginIdentity {
            id: self.manifest.id.clone(),
            source: self.source,
            publisher_id: self.manifest.publisher_id.clone(),
            approval_fingerprint: self.approval_fingerprint.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::plugin::manifest::{PluginManifestV1, PluginSource, APPROVAL_FINGERPRINT_NONE};

    use super::PluginRecord;

    const VALID_MANIFEST_JSON: &str = r#"{
        "schemaVersion": 1,
        "id": "com.example.alpha",
        "publisherId": "com.example",
        "publisher": "Example",
        "name": "Alpha",
        "description": "Metadata",
        "version": "1.0.0",
        "contributions": [],
        "requestedCapabilities": []
    }"#;

    const REORDERED_MANIFEST_JSON: &str = r#"{
        "requestedCapabilities": [],
        "description": "Metadata",
        "publisher": "Example",
        "id": "com.example.alpha",
        "version": "1.0.0",
        "schemaVersion": 1,
        "contributions": [],
        "name": "Alpha",
        "publisherId": "com.example"
    }"#;

    // Catches hashing raw JSON layout rather than the trusted manifest semantics.
    #[test]
    fn local_fingerprint_ignores_json_layout_but_binds_semantics() {
        let first: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST_JSON).unwrap();
        let reordered: PluginManifestV1 = serde_json::from_str(REORDERED_MANIFEST_JSON).unwrap();
        let first = PluginRecord::local_declarative(first).unwrap();
        let reordered = PluginRecord::local_declarative(reordered).unwrap();
        assert_eq!(
            first.approval_fingerprint(),
            reordered.approval_fingerprint()
        );

        let mut changed = reordered.manifest().clone();
        changed.name = "Changed name".to_owned();
        let changed = PluginRecord::local_declarative(changed).unwrap();
        assert_ne!(first.approval_fingerprint(), changed.approval_fingerprint());
    }

    // Catches built-ins acquiring an approval hash or identity fields detached from the manifest.
    #[test]
    fn built_in_record_exposes_a_stable_identity() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST_JSON).unwrap();
        let record = PluginRecord::built_in(manifest).unwrap();

        assert_eq!(record.source(), PluginSource::BuiltIn);
        assert_eq!(record.approval_fingerprint(), APPROVAL_FINGERPRINT_NONE);
        assert_eq!(record.identity().id.as_str(), "com.example.alpha");
        assert_eq!(record.identity().publisher_id.as_str(), "com.example");
        assert_eq!(record.identity().source, PluginSource::BuiltIn);
        assert_eq!(
            record.identity().approval_fingerprint,
            APPROVAL_FINGERPRINT_NONE
        );
    }
}

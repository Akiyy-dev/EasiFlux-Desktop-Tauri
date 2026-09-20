use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::plugin::contribution::PluginCommandContribution;
use crate::plugin::manifest::{
    PluginId, PluginManifest, PluginPublisherId, PluginSource, APPROVAL_FINGERPRINT_NONE,
    PLUGIN_MANIFEST_SCHEMA_VERSION_V1,
};

const LOCAL_FINGERPRINT_DOMAIN: &[u8] = b"EasiFlux.localDeclarative.manifest.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PluginRecord {
    manifest: PluginManifest,
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
    contributions: &'a [PluginCommandContribution],
    requested_capabilities: &'a [String],
}

impl<'a> From<&'a PluginManifest> for CanonicalManifestV1<'a> {
    fn from(manifest: &'a PluginManifest) -> Self {
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
    pub(crate) fn built_in(manifest: PluginManifest) -> Result<Self, String> {
        manifest.validate()?;
        if manifest.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION_V1 {
            return Err("built-in plugins require manifest schema v1".into());
        }
        Ok(Self {
            manifest,
            source: PluginSource::BuiltIn,
            approval_fingerprint: APPROVAL_FINGERPRINT_NONE.to_owned(),
        })
    }

    pub(crate) fn local_declarative(manifest: PluginManifest) -> Result<Self, String> {
        manifest.validate()?;
        let bytes = canonical_manifest_bytes(&manifest)?;
        let mut digest = Sha256::new();
        digest.update(LOCAL_FINGERPRINT_DOMAIN);
        digest.update(bytes);
        Ok(Self {
            manifest,
            source: PluginSource::LocalDeclarative,
            approval_fingerprint: format!("v1:sha256:{}", hex::encode(digest.finalize())),
        })
    }

    pub(crate) fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub(crate) fn canonical_manifest_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_manifest_bytes(&self.manifest)
    }

    pub(crate) fn source(&self) -> PluginSource {
        self.source
    }

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

fn canonical_manifest_bytes(manifest: &PluginManifest) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&CanonicalManifestV1::from(manifest))
        .map_err(|_| "manifest fingerprint failed".to_owned())
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

    // Catches v2 contributions inheriting the unhashed built-in trust path.
    #[test]
    fn built_in_record_rejects_v2_manifests() {
        let contribution = r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#;
        let json = format!(
            r#"{{"schemaVersion":2,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{contribution}],"requestedCapabilities":[]}}"#
        );
        let manifest: PluginManifestV1 = serde_json::from_str(&json).unwrap();

        assert!(PluginRecord::built_in(manifest).is_err());
    }

    // Catches the new v3 action surface inheriting the unhashed built-in trust path.
    #[test]
    fn built_in_record_rejects_v3_manifests() {
        let json = r#"{"schemaVersion":3,"id":"com.example.shortcuts","publisherId":"com.example","publisher":"Example","name":"Shortcuts","description":"Workspace shortcuts","version":"1.0.0","contributions":[{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":"charts"}}],"requestedCapabilities":[]}"#;
        let manifest: PluginManifestV1 = serde_json::from_str(json).unwrap();

        assert!(PluginRecord::built_in(manifest).is_err());
    }

    // Catches any change to the pre-v2 canonical identity envelope.
    #[test]
    fn v1_canonical_bytes_and_digest_are_golden() {
        let manifest: PluginManifestV1 = serde_json::from_str(VALID_MANIFEST_JSON).unwrap();
        let record = PluginRecord::local_declarative(manifest).unwrap();
        assert_eq!(
            record.canonical_manifest_bytes().unwrap(),
            br#"{"schemaVersion":1,"id":"com.example.alpha","publisherId":"com.example","publisher":"Example","name":"Alpha","description":"Metadata","version":"1.0.0","contributions":[],"requestedCapabilities":[]}"#
        );
        assert_eq!(
            record.approval_fingerprint(),
            "v1:sha256:2bb266f48b0c74e5a74708ea4442a56ad8fe583be81f93e27483a21d4e860ee3"
        );
    }

    // Catches hashing v2 JSON layout or omitting nested command semantics.
    #[test]
    fn v2_fingerprint_ignores_nested_key_order_and_binds_params() {
        let first = r#"{"schemaVersion":2,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}],"requestedCapabilities":[]}"#;
        let reordered = r#"{"requestedCapabilities":[],"contributions":[{"params":{"text":"Read-only guide","title":"Guide"},"actionId":"host.showInfo","title":"Guide","contributionId":"guide.overview","kind":"command"}],"version":"1.0.0","description":"Read-only guide","name":"Guide","publisher":"Example","publisherId":"com.example","id":"com.example.guide","schemaVersion":2}"#;
        let first = PluginRecord::local_declarative(serde_json::from_str(first).unwrap()).unwrap();
        let reordered =
            PluginRecord::local_declarative(serde_json::from_str(reordered).unwrap()).unwrap();
        assert_eq!(
            first.canonical_manifest_bytes().unwrap(),
            br#"{"schemaVersion":2,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}],"requestedCapabilities":[]}"#
        );
        assert_eq!(
            first.approval_fingerprint(),
            reordered.approval_fingerprint()
        );

        let mut changed = reordered.manifest().clone();
        let crate::plugin::contribution::PluginCommandParams::ShowInfo(params) =
            &mut changed.contributions[0].params
        else {
            panic!("expected showInfo params");
        };
        params.text = "Changed guide".into();
        let changed = PluginRecord::local_declarative(changed).unwrap();
        assert_ne!(first.approval_fingerprint(), changed.approval_fingerprint());
    }

    #[test]
    fn v2_and_v3_pre_compute_fingerprints_remain_golden() {
        for (document, digest) in [
            (
                r#"{"schemaVersion":2,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}],"requestedCapabilities":[]}"#,
                "v1:sha256:3305a75b0633ffb2f0b9c919848d9509c770c806c105266599c84c3a9bbb6368",
            ),
            (
                r#"{"schemaVersion":3,"id":"com.example.guide","publisherId":"com.example","publisher":"Example","name":"Guide","description":"Read-only guide","version":"1.0.0","contributions":[{"kind":"command","contributionId":"guide.overview","title":"Charts","actionId":"host.openPage","params":{"destination":"charts"}}],"requestedCapabilities":[]}"#,
                "v1:sha256:3afd36fa1fa0c177d82df718dcb24f126b34e7b759aa1620d404b8f97bc1f8f3",
            ),
        ] {
            let record =
                PluginRecord::local_declarative(serde_json::from_str(document).unwrap()).unwrap();
            assert_eq!(
                record.canonical_manifest_bytes().unwrap(),
                document.as_bytes()
            );
            assert_eq!(record.approval_fingerprint(), digest);
        }
    }

    #[test]
    fn v4_fingerprint_binds_guest_code_and_parameter_metadata() {
        let document = serde_json::json!({"schemaVersion":4,"id":"com.example.compute","publisherId":"com.example",
            "publisher":"Example","name":"Compute","description":"Compute","version":"1.0.0","requestedCapabilities":[],
            "contributions":[{"kind":"command","contributionId":"series.sma","title":"SMA","actionId":"sandbox.computeSeries",
                "params":crate::plugin::compute::tests::params(crate::plugin::compute::tests::SMA)}]});
        let original =
            PluginRecord::local_declarative(serde_json::from_value(document.clone()).unwrap())
                .unwrap();
        for (key, value) in [
            ("default", serde_json::json!(2)),
            ("label", serde_json::json!("Window")),
        ] {
            let mut changed = document.clone();
            changed["contributions"][0]["params"]["parameter"][key] = value;
            let record =
                PluginRecord::local_declarative(serde_json::from_value(changed).unwrap()).unwrap();
            assert_ne!(record.identity(), original.identity());
        }
        let mut changed = document;
        changed["contributions"][0]["params"]["moduleBase64"] = serde_json::json!("AGFzbQEAAAA=");
        let record =
            PluginRecord::local_declarative(serde_json::from_value(changed).unwrap()).unwrap();
        assert_ne!(record.identity(), original.identity());
    }
}

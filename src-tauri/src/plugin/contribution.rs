use serde::{Deserialize, Deserializer, Serialize};

use super::manifest::{deserialize_object, validate_display_text, PluginId};

const MAX_COMMAND_TITLE_LENGTH: usize = 80;
const MAX_COMMAND_TEXT_LENGTH: usize = 2_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginContributionKind {
    Command,
}

impl<'de> Deserialize<'de> for PluginContributionKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "command" => Ok(Self::Command),
            value => Err(serde::de::Error::unknown_variant(value, &["command"])),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum PluginCommandActionId {
    #[serde(rename = "host.showInfo")]
    HostShowInfo,
    #[serde(rename = "host.openPage")]
    HostOpenPage,
    #[serde(rename = "sandbox.computeSeries")]
    SandboxComputeSeries,
    #[serde(rename = "sandbox.accountWorkflow")]
    SandboxAccountWorkflow,
}

impl<'de> Deserialize<'de> for PluginCommandActionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "host.showInfo" => Ok(Self::HostShowInfo),
            "host.openPage" => Ok(Self::HostOpenPage),
            "sandbox.computeSeries" => Ok(Self::SandboxComputeSeries),
            "sandbox.accountWorkflow" => Ok(Self::SandboxAccountWorkflow),
            value => Err(serde::de::Error::unknown_variant(
                value,
                &["host.showInfo", "host.openPage", "sandbox.computeSeries"],
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum PluginPageDestination {
    #[serde(rename = "home")]
    Home,
    #[serde(rename = "trading")]
    Trading,
    #[serde(rename = "charts")]
    Charts,
    #[serde(rename = "settings.general")]
    SettingsGeneral,
    #[serde(rename = "settings.notifications")]
    SettingsNotifications,
    #[serde(rename = "settings.about")]
    SettingsAbout,
}

impl<'de> Deserialize<'de> for PluginPageDestination {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match String::deserialize(deserializer)?.as_str() {
            "home" => Ok(Self::Home),
            "trading" => Ok(Self::Trading),
            "charts" => Ok(Self::Charts),
            "settings.general" => Ok(Self::SettingsGeneral),
            "settings.notifications" => Ok(Self::SettingsNotifications),
            "settings.about" => Ok(Self::SettingsAbout),
            value => Err(serde::de::Error::unknown_variant(
                value,
                &[
                    "home",
                    "trading",
                    "charts",
                    "settings.general",
                    "settings.notifications",
                    "settings.about",
                ],
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoParams {
    pub title: String,
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginInfoParamsWire {
    title: String,
    text: String,
}

impl<'de> Deserialize<'de> for PluginInfoParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = deserialize_object::<D, PluginInfoParamsWire>(deserializer)?;
        let params = Self {
            title: wire.title,
            text: wire.text,
        };
        params.validate().map_err(serde::de::Error::custom)?;
        Ok(params)
    }
}

impl PluginInfoParams {
    pub fn validate(&self) -> Result<(), String> {
        validate_display_text(
            "command params title",
            &self.title,
            MAX_COMMAND_TITLE_LENGTH,
        )?;
        validate_display_text("command params text", &self.text, MAX_COMMAND_TEXT_LENGTH)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOpenPageParams {
    pub destination: PluginPageDestination,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginOpenPageParamsWire {
    destination: PluginPageDestination,
}

impl<'de> Deserialize<'de> for PluginOpenPageParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = deserialize_object::<D, PluginOpenPageParamsWire>(deserializer)?;
        Ok(Self {
            destination: wire.destination,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PluginCommandParams {
    ShowInfo(PluginInfoParams),
    OpenPage(PluginOpenPageParams),
    ComputeSeries(super::compute::PluginComputeParams),
    AccountWorkflow(super::workflow::PluginWorkflowParams),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandContribution {
    pub kind: PluginContributionKind,
    pub contribution_id: PluginId,
    pub title: String,
    pub action_id: PluginCommandActionId,
    pub params: PluginCommandParams,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PluginCommandContributionWire {
    kind: PluginContributionKind,
    contribution_id: PluginId,
    title: String,
    action_id: PluginCommandActionId,
    params: PluginCommandParams,
}

impl<'de> Deserialize<'de> for PluginCommandContribution {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = deserialize_object::<D, PluginCommandContributionWire>(deserializer)?;
        let contribution = Self {
            kind: wire.kind,
            contribution_id: wire.contribution_id,
            title: wire.title,
            action_id: wire.action_id,
            params: wire.params,
        };
        contribution.validate().map_err(serde::de::Error::custom)?;
        Ok(contribution)
    }
}

impl PluginCommandContribution {
    pub fn validate(&self) -> Result<(), String> {
        validate_display_text("command title", &self.title, MAX_COMMAND_TITLE_LENGTH)?;
        match (&self.action_id, &self.params) {
            (PluginCommandActionId::HostShowInfo, PluginCommandParams::ShowInfo(params)) => {
                params.validate()
            }
            (PluginCommandActionId::HostOpenPage, PluginCommandParams::OpenPage(_)) => Ok(()),
            (
                PluginCommandActionId::SandboxComputeSeries,
                PluginCommandParams::ComputeSeries(params),
            ) => params.validate(),
            (
                PluginCommandActionId::SandboxAccountWorkflow,
                PluginCommandParams::AccountWorkflow(params),
            ) => params.validate(),
            _ => Err("plugin command action does not match params".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::PluginManifestV1;
    use serde_json::{json, Value};

    const VALID: &str = r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#;
    const VALID_NAVIGATION: &str = r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":"charts"}}"#;

    #[test]
    fn workflow_v5_closed_contract_and_legacy_exclusion() {
        let action = json!({"kind":"command","contributionId":"account.inspect","title":"Inspect",
            "actionId":"sandbox.accountWorkflow", "params":{"runtime":"wasm-v1", "abi":"account-json-v1",
            "moduleBase64":"AGFzbQEAAAA=", "defaultInput":"{}"}});
        let mut document = manifest(5, vec![action]);
        document["requestedCapabilities"] = json!(["account.read", "trade.place"]);
        let parsed: PluginManifestV1 = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), document);
        for version in 1..=4 {
            let mut old = document.clone();
            old["schemaVersion"] = json!(version);
            assert!(serde_json::from_value::<PluginManifestV1>(old).is_err());
        }
        for capabilities in [
            json!([]),
            json!(["trade.place"]),
            json!(["account.read", "network"]),
            json!(["account.read", "account.read"]),
        ] {
            let mut invalid = document.clone();
            invalid["requestedCapabilities"] = capabilities;
            assert!(serde_json::from_value::<PluginManifestV1>(invalid).is_err());
        }
        for input in ["[]", "null", "{", "{}{}"] {
            let mut invalid = document.clone();
            invalid["contributions"][0]["params"]["defaultInput"] = json!(input);
            assert!(serde_json::from_value::<PluginManifestV1>(invalid).is_err());
        }
        document["contributions"] = json!([serde_json::from_str::<Value>(VALID).unwrap()]);
        assert!(serde_json::from_value::<PluginManifestV1>(document).is_err());
    }

    #[test]
    fn manifest_v4_compute_roundtrips_and_rejects_old_versions() {
        let compute = json!({"kind":"command","contributionId":"series.sma","title":"SMA",
            "actionId":"sandbox.computeSeries", "params":{"runtime":"wasm-v1", "abi":"series-f64-v1",
            "moduleBase64":"AGFzbQEAAAA=", "parameter":{"label":"Period","default":3,"min":1,"max":4096}}});
        let document = manifest(4, vec![compute.clone()]);
        let parsed: PluginManifestV1 = serde_json::from_value(document.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), document);
        for version in 1..=3 {
            assert!(serde_json::from_value::<PluginManifestV1>(manifest(
                version,
                vec![compute.clone()]
            ))
            .is_err());
        }
    }

    fn manifest(schema_version: u32, contributions: Vec<Value>) -> Value {
        json!({
            "schemaVersion": schema_version,
            "id": "com.example.guide",
            "publisherId": "com.example",
            "publisher": "Example",
            "name": "Guide",
            "description": "Read-only guide",
            "version": "1.0.0",
            "contributions": contributions,
            "requestedCapabilities": []
        })
    }

    // Catches permissive object, action, or parameter decoding at the contribution boundary.
    #[test]
    fn contribution_rejects_non_objects_unknown_or_duplicate_fields_and_unknown_literals() {
        serde_json::from_str::<PluginCommandContribution>(VALID).unwrap();
        let invalid = [
            r#"["command","guide.overview","Guide","host.showInfo",{"title":"Guide","text":"Read-only guide"}]"#,
            r#"{"kind":"menu","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":{"command":null},"contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.navigate","params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":{"host.showInfo":null},"params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","extra":true,"params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide","html":"<b>guide</b>"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":["Guide","Read-only guide"]}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide","text":"again"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","contributionId":"guide.other","title":"Guide","actionId":"host.showInfo","params":{"title":"Guide","text":"Read-only guide"}}"#,
        ];
        for document in invalid {
            assert!(
                serde_json::from_str::<PluginCommandContribution>(document).is_err(),
                "accepted {document}"
            );
        }
    }

    // Catches accepting navigation without the v3 typed action/parameter contract.
    #[test]
    fn contribution_accepts_allowlisted_navigation_and_rejects_inexact_navigation_params() {
        serde_json::from_str::<PluginCommandContribution>(VALID_NAVIGATION).unwrap();

        let invalid = [
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":"settings.account"}}"#,
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":"charts","title":"Mixed"}}"#,
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":["charts"]}"#,
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":"charts","destination":"trading"}}"#,
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"destination":{"page":"charts"}}}"#,
            r#"{"kind":"command","contributionId":"workspace.charts","title":"Open charts","actionId":"host.openPage","params":{"title":"Guide","text":"Read-only guide"}}"#,
            r#"{"kind":"command","contributionId":"guide.overview","title":"Guide","actionId":"host.showInfo","params":{"destination":"charts"}}"#,
        ];
        for document in invalid {
            assert!(
                serde_json::from_str::<PluginCommandContribution>(document).is_err(),
                "accepted {document}"
            );
        }
    }

    // Catches schema v3 admission drift or permitting v2 to use the navigation action.
    #[test]
    fn manifest_v3_accepts_mixed_commands_but_v2_rejects_navigation() {
        let info: Value = serde_json::from_str(VALID).unwrap();
        let navigation: Value = serde_json::from_str(VALID_NAVIGATION).unwrap();
        serde_json::from_value::<PluginManifestV1>(manifest(3, vec![navigation.clone(), info]))
            .unwrap();

        assert!(serde_json::from_value::<PluginManifestV1>(manifest(2, vec![navigation])).is_err());
    }

    // Keeps the shipped author example on the same strict production deserializer path.
    #[test]
    fn workspace_shortcuts_example_is_a_valid_v3_manifest() {
        let manifest: PluginManifestV1 = serde_json::from_str(include_str!(
            "../../../examples/plugins/workspace-shortcuts/manifest.json"
        ))
        .unwrap();

        assert_eq!(manifest.schema_version, 3);
        assert_eq!(
            manifest.id.as_str(),
            "com.easiflux.examples.workspace-shortcuts"
        );
        assert_eq!(manifest.contributions.len(), 4);
    }

    // Catches character-count limits, whitespace-only text, or skipping trusted validation.
    #[test]
    fn contribution_text_uses_utf8_byte_limits_and_existing_blank_rule() {
        let exact_title = format!("{}aa", "界".repeat(26));
        let exact_text = format!("{}aa", "界".repeat(666));
        assert_eq!(exact_title.len(), 80);
        assert_eq!(exact_text.len(), 2_000);
        let mut valid: Value = serde_json::from_str(VALID).unwrap();
        valid["title"] = json!(exact_title);
        valid["params"]["title"] = json!("x".repeat(80));
        valid["params"]["text"] = json!(exact_text);
        serde_json::from_value::<PluginCommandContribution>(valid).unwrap();

        for (field, value) in [
            ("title", "界".repeat(27)),
            ("title", " \u{feff}\t".to_owned()),
            ("params.title", "x".repeat(81)),
            ("params.title", "\u{feff}".to_owned()),
            ("params.text", "界".repeat(667)),
            ("params.text", " \u{feff}\n".to_owned()),
        ] {
            let mut document: Value = serde_json::from_str(VALID).unwrap();
            match field {
                "title" => document["title"] = json!(value),
                "params.title" => document["params"]["title"] = json!(value),
                "params.text" => document["params"]["text"] = json!(value),
                _ => unreachable!(),
            }
            assert!(
                serde_json::from_value::<PluginCommandContribution>(document).is_err(),
                "accepted invalid {field}"
            );
        }

        let mut trusted: PluginCommandContribution = serde_json::from_str(VALID).unwrap();
        let PluginCommandParams::ShowInfo(params) = &mut trusted.params else {
            panic!("expected showInfo params");
        };
        params.text = "\u{feff}".to_owned();
        assert!(trusted.validate().is_err());
    }

    // Catches schema/cardinality drift, duplicate IDs, or capabilities becoming an escape hatch.
    #[test]
    fn manifest_versions_enforce_command_cardinality_identity_and_no_capabilities() {
        let command: Value = serde_json::from_str(VALID).unwrap();
        let mut sixteen = Vec::new();
        for index in 0..16 {
            let mut item = command.clone();
            item["contributionId"] = json!(format!("guide.item-{index}"));
            sixteen.push(item);
        }
        serde_json::from_value::<PluginManifestV1>(manifest(2, sixteen)).unwrap();

        let mut with_capability = manifest(2, vec![command.clone()]);
        with_capability["requestedCapabilities"] = json!(["network"]);
        for invalid in [
            manifest(1, vec![command.clone()]),
            manifest(2, vec![]),
            manifest(2, vec![command.clone(); 17]),
            manifest(2, vec![command.clone(), command]),
            with_capability,
        ] {
            assert!(serde_json::from_value::<PluginManifestV1>(invalid).is_err());
        }
    }
}

use crate::plugin::{manifest::deserialize_object, workflow};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginStrategyParams {
    pub(crate) runtime: String,
    pub(crate) abi: String,
    pub(crate) module_base64: String,
    pub(crate) default_input: String,
}
impl<'de> Deserialize<'de> for PluginStrategyParams {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            runtime: String,
            abi: String,
            module_base64: String,
            default_input: String,
        }
        let w = deserialize_object::<D, Wire>(d)?;
        let p = Self {
            runtime: w.runtime,
            abi: w.abi,
            module_base64: w.module_base64,
            default_input: w.default_input,
        };
        p.validate().map_err(serde::de::Error::custom)?;
        Ok(p)
    }
}
impl PluginStrategyParams {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.runtime != "wasm-v1" || self.abi != "strategy-json-v1" {
            return Err("unsupported strategy ABI".into());
        }
        workflow::validate_input(&self.default_input)?;
        self.module_bytes().map(|_| ())
    }
    pub(crate) fn module_bytes(&self) -> Result<Vec<u8>, String> {
        workflow::PluginWorkflowParams {
            runtime: self.runtime.clone(),
            abi: "account-json-v1".into(),
            module_base64: self.module_base64.clone(),
            default_input: self.default_input.clone(),
        }
        .module_bytes()
    }
}
pub(crate) fn validate_capabilities(values: &[String]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for c in values {
        if !(workflow::CAPABILITIES.contains(&c.as_str()) || c == "strategy.run") || !seen.insert(c)
        {
            return Err("invalid strategy capability".into());
        }
    }
    if !values.iter().any(|c| c == "account.read") || !values.iter().any(|c| c == "strategy.run") {
        return Err("mandatory strategy capability missing".into());
    }
    Ok(())
}

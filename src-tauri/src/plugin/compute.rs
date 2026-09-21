//! Closed numerical guest contract. No host capabilities are represented here.
use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use wasmi::{
    CompilationMode, Config, EnforcedLimits, Engine, Linker, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc, TypedResumableCall, WasmParams, WasmResults,
};

use super::manifest::{deserialize_object, validate_display_text};

pub const MAX_MODULE_BYTES: usize = 8192;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) mod sandbox;
pub(crate) mod slot;
pub(crate) use sandbox::execute_guest;

pub(crate) fn error(code: &'static str) -> AppError {
    AppError::Plugin {
        code,
        message: "插件计算请求未完成",
        diagnostic: None,
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputeRequest {
    pub request_id: String,
    pub plugin_id: String,
    pub contribution_id: String,
    pub expected_catalog_generation: String,
    pub expected_revision: String,
    pub values: Vec<f64>,
    pub parameter: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeResult {
    pub schema_version: u8,
    pub request_id: String,
    pub plugin_id: String,
    pub contribution_id: String,
    pub catalog_generation: String,
    pub revision: String,
    pub value: f64,
    pub input_count: usize,
    pub parameter: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelComputeResult {
    pub schema_version: u8,
    pub request_id: String,
    pub cancelled: bool,
}

pub(crate) fn validate_request_id(id: &str) -> AppResult<()> {
    if !(1..=64).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(error("plugin_compute_invalid_request"));
    }
    Ok(())
}

impl ComputeRequest {
    pub(crate) fn validate(&self) -> AppResult<()> {
        use super::manifest::PluginId;
        validate_request_id(&self.request_id)?;
        for id in [&self.plugin_id, &self.contribution_id] {
            PluginId::parse(id).map_err(|_| error("plugin_compute_invalid_request"))?;
        }
        for counter in [&self.expected_catalog_generation, &self.expected_revision] {
            if counter
                .parse::<u64>()
                .ok()
                .is_none_or(|value| value.to_string() != *counter)
            {
                return Err(error("plugin_compute_invalid_request"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComputeParameter {
    pub label: String,
    pub default: i32,
    pub min: i32,
    pub max: i32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginComputeParams {
    pub runtime: String,
    pub abi: String,
    pub module_base64: String,
    pub parameter: ComputeParameter,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParamsWire {
    runtime: String,
    abi: String,
    module_base64: String,
    #[serde(deserialize_with = "deserialize_object")]
    parameter: ComputeParameter,
}

impl<'de> Deserialize<'de> for PluginComputeParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = deserialize_object::<D, ParamsWire>(deserializer)?;
        let result = Self {
            runtime: wire.runtime,
            abi: wire.abi,
            module_base64: wire.module_base64,
            parameter: wire.parameter,
        };
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}

impl PluginComputeParams {
    pub fn validate(&self) -> Result<(), String> {
        if self.runtime != "wasm-v1" || self.abi != "series-f64-v1" {
            return Err("unsupported compute runtime or ABI".into());
        }
        validate_display_text("parameter label", &self.parameter.label, 80)?;
        let p = &self.parameter;
        if p.min < -1_000_000 || p.max > 1_000_000 || p.min > p.default || p.default > p.max {
            return Err("invalid compute parameter bounds".into());
        }
        self.module_bytes().map(|_| ())
    }

    pub(crate) fn module_bytes(&self) -> Result<Vec<u8>, String> {
        if self.module_base64.len() > MAX_MODULE_BYTES.div_ceil(3) * 4 {
            return Err("compute module too large".into());
        }
        let bytes = STANDARD
            .decode(&self.module_base64)
            .map_err(|_| "invalid compute module encoding")?;
        if bytes.len() > MAX_MODULE_BYTES
            || !bytes.starts_with(b"\0asm\x01\0\0\0")
            || STANDARD.encode(&bytes) != self.module_base64
        {
            return Err("invalid compute module header or encoding".into());
        }
        Ok(bytes)
    }
}

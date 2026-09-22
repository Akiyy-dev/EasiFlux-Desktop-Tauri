use super::*;
use crate::error::AppResult;
use crate::{
    plugin::workflow,
    storage::safe_plugin_document::{
        outcome_satisfies_destructive_barrier, CandidateBytes, SafeDocumentNames,
        SafePluginDocument,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

pub(crate) const MAX_STORE_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OwnedOrder {
    pub(crate) submission_id: String,
    pub(crate) cancel_requested: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PendingAction {
    pub(crate) sequence: String,
    pub(crate) action: StrategyAction,
    pub(crate) submission_id: Option<String>,
    pub(crate) state: serde_json::Value,
    pub(crate) message: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunRecord {
    pub(crate) view: StrategyRunView,
    pub(crate) fingerprint: String,
    pub(crate) scope: String,
    pub(crate) state: serde_json::Value,
    pub(crate) owned: BTreeMap<String, OwnedOrder>,
    pub(crate) pending: Option<PendingAction>,
    pub(crate) last_observed_at_ms: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StrategyDocument {
    pub(crate) schema_version: u8,
    pub(crate) revision: String,
    pub(crate) runs: Vec<RunRecord>,
}
macro_rules! object {
    ($t:ident { $($(#[$attr:meta])* $field:ident:$ty:ty),* })=>{
        impl<'de> Deserialize<'de> for $t {fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error>{
            #[derive(Deserialize)] #[serde(rename_all="camelCase",deny_unknown_fields)] struct Wire { $($(#[$attr])* $field:$ty),* }
            let w=crate::plugin::manifest::deserialize_object::<D,Wire>(d)?;Ok(Self{$($field:w.$field),*})
        }}
    }
}
object!(OwnedOrder {
    submission_id: String,
    cancel_requested: bool
});
object!(PendingAction{sequence:String,action:StrategyAction,#[serde(deserialize_with="nullable")] submission_id:Option<String>,state:serde_json::Value,message:String});
object!(RunRecord{view:StrategyRunView,fingerprint:String,scope:String,state:serde_json::Value,#[serde(deserialize_with="owned_orders")] owned:BTreeMap<String,OwnedOrder>,#[serde(deserialize_with="nullable")] pending:Option<PendingAction>,last_observed_at_ms:String});
object!(StrategyDocument{schema_version:u8,revision:String,runs:Vec<RunRecord>});
fn nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
fn owned_orders<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, OwnedOrder>, D::Error> {
    struct UniqueOrders;
    impl<'de> serde::de::Visitor<'de> for UniqueOrders {
        type Value = BTreeMap<String, OwnedOrder>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique owned order identities")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut orders = BTreeMap::new();
            while let Some((id, order)) = map.next_entry()? {
                if orders.len() >= 1000 || orders.insert(id, order).is_some() {
                    return Err(serde::de::Error::custom("ambiguous owned order identity"));
                }
            }
            Ok(orders)
        }
    }
    d.deserialize_map(UniqueOrders)
}
impl Default for StrategyDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: "0".into(),
            runs: vec![],
        }
    }
}
pub(crate) trait StrategyStore: Send + Sync {
    fn load(&self) -> AppResult<StrategyDocument>;
    fn save(&self, doc: &StrategyDocument) -> AppResult<()>;
}
pub(crate) struct UnavailableStrategyStore;
pub(crate) fn production_strategy_store() -> std::sync::Arc<dyn StrategyStore> {
    match FileStrategyStore::production() {
        Ok(store) => std::sync::Arc::new(store),
        Err(_) => std::sync::Arc::new(UnavailableStrategyStore),
    }
}
impl StrategyStore for UnavailableStrategyStore {
    fn load(&self) -> AppResult<StrategyDocument> {
        Err(error("plugin_strategy_storage_unavailable"))
    }
    fn save(&self, _: &StrategyDocument) -> AppResult<()> {
        Err(error("plugin_strategy_storage_unavailable"))
    }
}
impl StrategyDocument {
    pub(crate) fn validate(&self) -> AppResult<()> {
        if self.schema_version != 1 || self.runs.len() > 32 {
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        workflow::counter(&self.revision)?;
        let mut ids = std::collections::BTreeSet::new();
        let mut requests = std::collections::BTreeSet::new();
        for r in &self.runs {
            r.view.validate()?;
            if workflow::counter(&r.last_observed_at_ms)?
                < workflow::counter(&r.view.started_at_ms)?
            {
                return Err(error("plugin_strategy_storage_unavailable"));
            }
            validate_state(&r.state)?;
            if !ids.insert(&r.view.run_id)
                || !requests.insert(&r.view.request_id)
                || r.scope.len() != 64
                || !r.scope.bytes().all(|b| b.is_ascii_hexdigit())
                || r.fingerprint.len() != 74
                || !r
                    .fingerprint
                    .strip_prefix("v1:sha256:")
                    .is_some_and(|s| s.bytes().all(|b| b.is_ascii_hexdigit()))
                || r.owned.len() > r.view.actions_submitted as usize
            {
                return Err(error("plugin_strategy_storage_unavailable"));
            }
            let mut submissions = std::collections::BTreeSet::new();
            for (id, o) in &r.owned {
                if !workflow::bounded_id(id)
                    || uuid::Uuid::parse_str(&o.submission_id).is_err()
                    || !submissions.insert(&o.submission_id)
                {
                    return Err(error("plugin_strategy_storage_unavailable"));
                }
            }
            if let Some(p) = &r.pending {
                validate_state(&p.state)?;
                if p.sequence != r.view.sequence
                    || p.message.len() > 2000
                    || r.view.actions_submitted == 0
                {
                    return Err(error("plugin_strategy_storage_unavailable"));
                }
                match &p.action {
                    StrategyAction::PlaceOrder { order } => {
                        workflow::WorkflowOutput::PlaceOrder {
                            order: order.clone(),
                        }
                        .validate(
                            &r.view.symbol,
                            &r.view.capabilities,
                            None,
                        )?;
                        if !p
                            .submission_id
                            .as_ref()
                            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
                            || quantity(&order.qty)? > quantity(&r.view.policy.max_order_qty)?
                            || (r.view.policy.reduce_only && !order.reduce_only)
                        {
                            return Err(error("plugin_strategy_storage_unavailable"));
                        }
                    }
                    StrategyAction::CancelOrder { order } => {
                        if order.symbol != r.view.symbol
                            || p.submission_id.is_some()
                            || !r.owned.contains_key(&order.order_id)
                        {
                            return Err(error("plugin_strategy_storage_unavailable"));
                        }
                    }
                    _ => return Err(error("plugin_strategy_storage_unavailable")),
                }
            }
        }
        Ok(())
    }
}
pub(crate) struct FileStrategyStore {
    file: SafePluginDocument,
    transaction: Mutex<()>,
}
impl FileStrategyStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            file: SafePluginDocument::new(root, SafeDocumentNames::strategy(), MAX_STORE_BYTES),
            transaction: Mutex::new(()),
        }
    }
    pub(crate) fn production() -> AppResult<Self> {
        Ok(Self::new(
            dirs::config_dir()
                .ok_or_else(|| error("plugin_strategy_storage_unavailable"))?
                .join(crate::models::config::APP_NAME)
                .join("strategies"),
        ))
    }
    fn read(&self) -> AppResult<StrategyDocument> {
        let mut versions = BTreeMap::new();
        for candidate in self
            .file
            .load_all_candidates()
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?
        {
            let bytes = match candidate {
                CandidateBytes::Missing => continue,
                CandidateBytes::Oversized => {
                    return Err(error("plugin_strategy_storage_unavailable"))
                }
                CandidateBytes::Bounded(bytes) => bytes,
            };
            let doc: StrategyDocument = serde_json::from_slice(&bytes)
                .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
            doc.validate()
                .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
            if versions
                .insert(workflow::counter(&doc.revision)?, doc.clone())
                .is_some_and(|old| old != doc)
            {
                return Err(error("plugin_strategy_storage_unavailable"));
            }
        }
        Ok(versions.pop_last().map(|(_, doc)| doc).unwrap_or_default())
    }
}
impl StrategyStore for FileStrategyStore {
    fn load(&self) -> AppResult<StrategyDocument> {
        let _guard = self
            .transaction
            .lock()
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        self.read()
    }
    fn save(&self, doc: &StrategyDocument) -> AppResult<()> {
        let _guard = self
            .transaction
            .lock()
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        doc.validate()
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        let old = self.read()?;
        if workflow::counter(&doc.revision)?
            != workflow::counter(&old.revision)?
                .checked_add(1)
                .ok_or_else(|| error("plugin_strategy_storage_unavailable"))?
        {
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        let bytes =
            serde_json::to_vec(doc).map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        let previous =
            serde_json::to_vec(&old).map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        let outcome = self
            .file
            .persist(Some(&previous), &bytes)
            .map_err(|_| error("plugin_strategy_storage_unavailable"))?;
        if !outcome_satisfies_destructive_barrier(outcome) {
            return Err(error("plugin_strategy_storage_unavailable"));
        }
        Ok(())
    }
}

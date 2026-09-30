use crate::plugin::manifest::PluginManifest;
use serde_json::{json, Value};
mod contract;
mod store;
mod supervisor;

fn manifest() -> Value {
    json!({"schemaVersion":6,"id":"com.example.strategy","publisherId":"com.example",
        "publisher":"Example","name":"Strategy","description":"Synthetic strategy","version":"1.0.0",
        "requestedCapabilities":["account.read","strategy.run","trade.place"],
        "contributions":[{"kind":"command","contributionId":"strategy.run","title":"Run",
        "actionId":"sandbox.strategy","params":{"runtime":"wasm-v1","abi":"strategy-json-v1",
        "moduleBase64":"AGFzbQEAAAA=","defaultInput":"{}"}}]})
}

// Catches rejecting the new isolated ABI or serializing it as a v5 workflow.
#[test]
fn v6_roundtrips_closed_strategy_contract() {
    let value = manifest();
    let parsed: PluginManifest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
}

#[test]
fn v7_requires_explicit_management_dependencies_without_extending_v6() {
    for (capability, dependencies) in [
        ("trade.amend", &["trade.place", "orders.read", "market.read"][..]),
        ("trade.protect", &["trade.place", "market.read"][..]),
    ] {
        let mut valid = manifest();
        valid["schemaVersion"] = json!(7);
        let mut caps = vec!["account.read", "strategy.run", capability];
        caps.extend_from_slice(dependencies);
        valid["requestedCapabilities"] = json!(caps);
        assert!(serde_json::from_value::<PluginManifest>(valid.clone()).is_ok());
        for missing in dependencies {
            let mut invalid = valid.clone();
            invalid["requestedCapabilities"] = json!(caps.iter().filter(|c| *c != missing).collect::<Vec<_>>());
            assert!(serde_json::from_value::<PluginManifest>(invalid).is_err(), "missing {missing}");
        }
        valid["schemaVersion"] = json!(6);
        assert!(serde_json::from_value::<PluginManifest>(valid).is_err());
    }
}

// Catches granting unattended authority to any legacy manifest.
#[test]
fn v5_cannot_acquire_unattended_authority() {
    for version in 1..=5 {
        let mut value = manifest();
        value["schemaVersion"] = json!(version);
        assert!(serde_json::from_value::<PluginManifest>(value).is_err());
    }
}

#[test]
fn strategy_abi_unknown_fields_and_missing_mandatory_grants_rejected() {
    for (path, value) in [
        ("/contributions/0/params/abi", json!("account-json-v1")),
        ("/contributions/0/params/runtime", json!("native")),
        ("/contributions/0/params/defaultInput", json!("[]")),
        ("/requestedCapabilities", json!(["account.read"])),
        (
            "/requestedCapabilities",
            json!(["account.read", "strategy.run", "strategy.run"]),
        ),
        (
            "/contributions/0/actionId",
            json!("sandbox.accountWorkflow"),
        ),
    ] {
        let mut invalid = manifest();
        *invalid.pointer_mut(path).unwrap() = value;
        assert!(
            serde_json::from_value::<PluginManifest>(invalid).is_err(),
            "accepted {path}"
        );
    }
    let mut invalid = manifest();
    invalid["contributions"][0]["params"]["network"] = json!(true);
    assert!(serde_json::from_value::<PluginManifest>(invalid).is_err());
}

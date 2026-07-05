use serde_json::Value;

use crate::events::EventEmitter;

use super::mapper::list_envelope_meta;
use super::response::{describe_data_shape, first_object_keys, payload_has_content};

pub fn warn_if_parse_empty(
    emitter: &EventEmitter,
    endpoint: &str,
    payload: &Value,
    parsed_count: usize,
) {
    if parsed_count > 0 || !payload_has_content(payload) {
        return;
    }
    let (data_type, data_keys) = describe_data_shape(payload);
    let meta = list_envelope_meta(payload);
    let first_keys = first_object_keys(payload);
    emitter.emit_log(
        "warn",
        &format!(
            "{endpoint} 响应成功但解析为空: envelope={}, data_type={data_type}, data_keys=[{}], first_item_keys=[{}]",
            meta.hint,
            data_keys.join(","),
            first_keys.join(",")
        ),
    );
}

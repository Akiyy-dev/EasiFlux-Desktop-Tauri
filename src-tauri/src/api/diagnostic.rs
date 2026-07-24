use serde_json::Value;

use crate::events::EventEmitter;

use super::mapper::list_envelope_meta;
use super::response::{
    describe_data_shape, first_object_keys, payload_has_content, ListEnvelopeMeta,
};

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
            "{endpoint} 响应成功但解析为空：响应结构={}，数据类型={data_type}，数据字段=[{}]，首项字段=[{}]",
            meta.hint,
            data_keys.join(","),
            first_keys.join(",")
        ),
    );
}

pub fn warn_if_raw_parsed_mismatch(
    emitter: &EventEmitter,
    endpoint: &str,
    meta: &ListEnvelopeMeta,
    parsed_count: usize,
) {
    if meta.raw_count == parsed_count {
        return;
    }
    emitter.emit_log(
        "warn",
        &format!(
            "{endpoint} API 返回 {} 条原始记录，但仅解析出 {} 条（响应结构={}）",
            meta.raw_count, parsed_count, meta.hint
        ),
    );
}

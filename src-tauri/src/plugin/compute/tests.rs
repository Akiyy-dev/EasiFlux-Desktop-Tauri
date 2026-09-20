use super::*;
use serde_json::json;
use std::sync::atomic::AtomicBool;

pub(crate) const SMA: &str = r#"(module
  (memory (export "memory") 1 16)
  (func (export "alloc") (param i32) (result i32) i32.const 0)
  (func (export "run") (param $ptr i32) (param $count i32) (param $period f64) (result f64)
    (local $i i32) (local $sum f64)
    local.get $count local.get $period i32.trunc_f64_s i32.sub local.set $i
    (loop $next
      local.get $sum local.get $ptr local.get $i i32.const 8 i32.mul i32.add f64.load f64.add local.set $sum
      local.get $i i32.const 1 i32.add local.tee $i local.get $count i32.lt_s br_if $next)
    local.get $sum local.get $period f64.div))"#;

pub(crate) fn params(wat: &str) -> PluginComputeParams {
    PluginComputeParams {
        runtime: "wasm-v1".into(),
        abi: "series-f64-v1".into(),
        module_base64: STANDARD.encode(wat::parse_str(wat).unwrap()),
        parameter: ComputeParameter {
            label: "Period".into(),
            default: 3,
            min: 1,
            max: 4096,
        },
    }
}

fn code(result: crate::error::AppResult<f64>) -> String {
    serde_json::to_value(result.unwrap_err()).unwrap()["code"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn actual_sma_guest_computes_supplied_series() {
    assert_eq!(
        execute_guest(
            &params(SMA),
            &[1., 2., 3., 4., 5.],
            3.,
            &AtomicBool::new(false)
        )
        .unwrap(),
        4.
    );
    assert_eq!(
        execute_guest(
            &params(SMA),
            &[10., 20., 30., 40.],
            2.,
            &AtomicBool::new(false)
        )
        .unwrap(),
        35.
    );
}

#[test]
fn strict_compute_metadata_rejects_alternate_shapes_and_encodings() {
    let valid = serde_json::to_value(params(SMA)).unwrap();
    for (path, replacement) in [
        ("runtime", json!("wasi")),
        ("abi", json!("other")),
        ("moduleBase64", json!("AGFzbQEAAAA")),
        ("moduleBase64", json!("AGFzbQEAAAB=")),
        ("moduleBase64", json!("")),
        ("moduleBase64", json!("AAAA")),
        ("parameter", json!(["Period", 3, 1, 4096])),
        (
            "parameter",
            json!({"label":" ","default":3,"min":1,"max":4096}),
        ),
        (
            "parameter",
            json!({"label":"P","default":3.5,"min":1,"max":4096}),
        ),
        (
            "parameter",
            json!({"label":"P","default":0,"min":1,"max":4096}),
        ),
        (
            "parameter",
            json!({"label":"P","default":3,"min":-1000001,"max":4096}),
        ),
        (
            "parameter",
            json!({"label":"P","default":3,"min":1,"max":1000001}),
        ),
    ] {
        let mut bad = valid.clone();
        bad[path] = replacement;
        assert!(
            serde_json::from_value::<PluginComputeParams>(bad).is_err(),
            "accepted {path}"
        );
    }
    let document = serde_json::to_string(&valid).unwrap();
    assert!(serde_json::from_str::<PluginComputeParams>(
        &document.replace("\"min\":1", "\"min\":1,\"min\":2")
    )
    .is_err());
    assert!(serde_json::from_str::<PluginComputeParams>(
        &document.replace("\"runtime\":", "\"extra\":0,\"runtime\":")
    )
    .is_err());
}

#[test]
fn rejects_import_start_bad_abi_pointer_nonfinite_and_excessive_memory() {
    let fixtures = [
        (
            "(module (import \"env\" \"f\" (func)))".to_owned(),
            "invalid_module",
        ),
        (
            "(module (func $start) (start $start))".to_owned(),
            "invalid_module",
        ),
        (
            "(module (memory (export \"memory\") 1))".to_owned(),
            "invalid_abi",
        ),
        (SMA.replace("i32.const 0)", "i32.const -1)"), "memory"),
        (SMA.replace("i32.const 0)", "i32.const 65535)"), "memory"),
        (SMA.replace("1 16)", "17 17)"), "invalid_module"),
        (
            SMA.replace("local.get $sum local.get $period f64.div", "f64.const nan"),
            "invalid_output",
        ),
        (
            SMA.replace("local.get $sum local.get $period f64.div", "unreachable"),
            "trap",
        ),
    ];
    for (wat, want) in fixtures {
        assert_eq!(
            code(execute_guest(
                &params(&wat),
                &[1., 2., 3.],
                3.,
                &AtomicBool::new(false)
            )),
            format!("plugin_compute_{want}")
        );
    }
    for input in [vec![], vec![f64::NAN], vec![f64::INFINITY], vec![0.; 4097]] {
        assert_eq!(
            code(execute_guest(
                &params(SMA),
                &input,
                3.,
                &AtomicBool::new(false)
            )),
            "plugin_compute_invalid_input"
        );
    }
    for parameter in [f64::NAN, f64::INFINITY, 0., 4097.] {
        assert_eq!(
            code(execute_guest(
                &params(SMA),
                &[1.],
                parameter,
                &AtomicBool::new(false)
            )),
            "plugin_compute_invalid_parameter"
        );
    }
}

#[test]
fn infinite_guest_exits_on_budget_and_pre_cancelled_guest_never_runs() {
    let looping = SMA.replace(
        "local.get $sum local.get $period f64.div",
        "(loop $forever br $forever) f64.const 0",
    );
    assert_eq!(
        code(execute_guest(
            &params(&looping),
            &[1., 2., 3.],
            3.,
            &AtomicBool::new(false)
        )),
        "plugin_compute_budget"
    );
    assert_eq!(
        code(execute_guest(
            &params(SMA),
            &[1., 2., 3.],
            3.,
            &AtomicBool::new(true)
        )),
        "plugin_compute_cancelled"
    );
}

#[test]
fn structural_limits_and_abi_are_checked_before_invocation() {
    let invalid = [
        SMA.replace("(memory", "(table 0 funcref) (memory"),
        SMA.replace("(memory", "(memory 1) (memory"),
        SMA.replace("(memory", &format!("{} (memory", "(func)".repeat(129))),
        SMA.replace(
            "(memory",
            &format!("{} (memory", "(global i32 (i32.const 0))".repeat(65)),
        ),
        SMA.replace(
            "(local $i i32)",
            &format!("(local {}) (local $i i32)", "i32 ".repeat(257)),
        ),
        SMA.replace(
            "(memory",
            "(import \"wasi_snapshot_preview1\" \"fd_write\" (func)) (memory",
        ),
    ];
    for wat in invalid {
        assert_eq!(
            code(execute_guest(
                &params(&wat),
                &[1., 2., 3.],
                3.,
                &AtomicBool::new(false)
            )),
            "plugin_compute_invalid_module"
        );
    }
    let wrong_abi = SMA.replace(
        "(param i32) (result i32) i32.const 0",
        "(param f64) (result i32) i32.const 0",
    );
    assert_eq!(
        code(execute_guest(
            &params(&wrong_abi),
            &[1.],
            1.,
            &AtomicBool::new(false)
        )),
        "plugin_compute_invalid_abi"
    );
}

#[test]
fn fresh_memory_each_run_and_fractional_parameters_are_supported() {
    let wat = r#"(module (memory (export "memory") 1 16)
        (func (export "alloc") (param i32) (result i32) i32.const 8)
        (func (export "run") (param i32 i32 f64) (result f64)
            i32.const 0 i32.const 0 f64.load local.get 2 f64.add f64.store
            i32.const 0 f64.load))"#;
    for _ in 0..2 {
        assert_eq!(
            execute_guest(&params(wat), &[2.], 1.5, &AtomicBool::new(false)).unwrap(),
            1.5
        );
    }
    let grow = SMA
        .replace("i32.const 0)", "i32.const 17 memory.grow drop i32.const 0)")
        .replace(
            "local.get $sum local.get $period f64.div",
            "memory.size f64.convert_i32_u",
        );
    assert_eq!(
        execute_guest(&params(&grow), &[1.], 1., &AtomicBool::new(false)).unwrap(),
        1.
    );
}

#[test]
fn allocation_and_run_share_a_single_fuel_budget() {
    let both = r#"(module (memory (export "memory") 1)
        (func $burn (local $n i32) i32.const 300000 local.set $n
            (loop $l local.get $n i32.const 1 i32.sub local.tee $n br_if $l))
        (func (export "alloc") (param i32) (result i32) call $burn i32.const 0)
        (func (export "run") (param i32 i32 f64) (result f64) call $burn f64.const 1))"#;
    assert_eq!(
        code(execute_guest(
            &params(both),
            &[1.],
            1.,
            &AtomicBool::new(false)
        )),
        "plugin_compute_budget"
    );
    let allocation_only = both.replace(
        "(result f64) call $burn f64.const 1",
        "(result f64) f64.const 1",
    );
    assert_eq!(
        execute_guest(
            &params(&allocation_only),
            &[1.],
            1.,
            &AtomicBool::new(false)
        )
        .unwrap(),
        1.
    );
}

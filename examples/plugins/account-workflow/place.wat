(module
  (memory (export "memory") 1 16)

  (func (export "alloc") (param $bytes i32) (result i32)
    local.get $bytes
    i32.const 65535
    i32.add
    i32.const 16
    i32.shr_u
    memory.grow
    i32.const 65536
    i32.mul)

  ;; This deliberately returns the user's strict JSON object. The host still
  ;; validates and canonicalizes every order field before offering confirmation.
  (func (export "run")
    (param $context_ptr i32) (param $context_len i32)
    (param $input_ptr i32) (param $input_len i32)
    (result i64)
    local.get $input_ptr
    i64.extend_i32_u
    i64.const 32
    i64.shl
    local.get $input_len
    i64.extend_i32_u
    i64.or))

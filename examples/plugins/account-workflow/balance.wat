(module
  ;; The first page holds immutable output text and scratch output. Every host
  ;; allocation grows fresh pages, so context and input can never overlap.
  (memory (export "memory") 1 16)
  (data (i32.const 16384) "{\"kind\":\"display\",\"text\":\"Available balance: ")

  (func (export "alloc") (param $bytes i32) (result i32)
    local.get $bytes
    i32.const 65535
    i32.add
    i32.const 16
    i32.shr_u
    memory.grow
    i32.const 65536
    i32.mul)

  (func $matches_available (param $ptr i32) (result i32)
    local.get $ptr i32.load8_u i32.const 34 i32.eq
    local.get $ptr i32.load8_u offset=1 i32.const 97 i32.eq i32.and
    local.get $ptr i32.load8_u offset=2 i32.const 118 i32.eq i32.and
    local.get $ptr i32.load8_u offset=3 i32.const 97 i32.eq i32.and
    local.get $ptr i32.load8_u offset=4 i32.const 105 i32.eq i32.and
    local.get $ptr i32.load8_u offset=5 i32.const 108 i32.eq i32.and
    local.get $ptr i32.load8_u offset=6 i32.const 97 i32.eq i32.and
    local.get $ptr i32.load8_u offset=7 i32.const 98 i32.eq i32.and
    local.get $ptr i32.load8_u offset=8 i32.const 108 i32.eq i32.and
    local.get $ptr i32.load8_u offset=9 i32.const 101 i32.eq i32.and
    local.get $ptr i32.load8_u offset=10 i32.const 34 i32.eq i32.and
    local.get $ptr i32.load8_u offset=11 i32.const 58 i32.eq i32.and
    local.get $ptr i32.load8_u offset=12 i32.const 34 i32.eq i32.and)

  (func (export "run")
    (param $context_ptr i32) (param $context_len i32)
    (param $input_ptr i32) (param $input_len i32)
    (result i64)
    (local $scan i32) (local $value_start i32) (local $value_end i32)
    (local $value_len i32) (local $copy i32) (local $output_len i32)

    (block $value_found
      (loop $scan_value
        local.get $scan
        i32.const 13
        i32.add
        local.get $context_len
        i32.gt_u
        if unreachable end
        local.get $context_ptr
        local.get $scan
        i32.add
        call $matches_available
        if
          local.get $context_ptr
          local.get $scan
          i32.add
          i32.const 13
          i32.add
          local.set $value_start
          br $value_found
        end
        local.get $scan
        i32.const 1
        i32.add
        local.set $scan
        br $scan_value))

    local.get $value_start
    local.set $value_end
    (block $end_found
      (loop $scan_end
        local.get $value_end
        local.get $context_ptr
        local.get $context_len
        i32.add
        i32.ge_u
        if unreachable end
        local.get $value_end
        i32.load8_u
        i32.const 34
        i32.eq
        br_if $end_found
        local.get $value_end
        i32.const 1
        i32.add
        local.set $value_end
        br $scan_end))

    local.get $value_end
    local.get $value_start
    i32.sub
    local.tee $value_len
    i32.const 64
    i32.gt_u
    if unreachable end

    (block $copy_done
      (loop $copy_value
        local.get $copy
        local.get $value_len
        i32.ge_u
        br_if $copy_done
        i32.const 16429
        local.get $copy
        i32.add
        local.get $value_start
        local.get $copy
        i32.add
        i32.load8_u
        i32.store8
        local.get $copy
        i32.const 1
        i32.add
        local.set $copy
        br $copy_value))

    i32.const 16429
    local.get $value_len
    i32.add
    i32.const 34
    i32.store8
    i32.const 16430
    local.get $value_len
    i32.add
    i32.const 125
    i32.store8
    i32.const 47
    local.get $value_len
    i32.add
    local.set $output_len

    i64.const 16384
    i64.const 32
    i64.shl
    local.get $output_len
    i64.extend_i32_u
    i64.or))

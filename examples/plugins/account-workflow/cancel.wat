(module
  (memory (export "memory") 1 16)
  (data (i32.const 8192) "\",\"orderId\":\"")
  (data (i32.const 16384) "{\"kind\":\"cancelOrder\",\"order\":{\"symbol\":\"")

  (func (export "alloc") (param $bytes i32) (result i32)
    local.get $bytes
    i32.const 65535
    i32.add
    i32.const 16
    i32.shr_u
    memory.grow
    i32.const 65536
    i32.mul)

  (func $matches_symbol (param $ptr i32) (result i32)
    local.get $ptr i32.load8_u i32.const 34 i32.eq
    local.get $ptr i32.load8_u offset=1 i32.const 115 i32.eq i32.and
    local.get $ptr i32.load8_u offset=2 i32.const 121 i32.eq i32.and
    local.get $ptr i32.load8_u offset=3 i32.const 109 i32.eq i32.and
    local.get $ptr i32.load8_u offset=4 i32.const 98 i32.eq i32.and
    local.get $ptr i32.load8_u offset=5 i32.const 111 i32.eq i32.and
    local.get $ptr i32.load8_u offset=6 i32.const 108 i32.eq i32.and
    local.get $ptr i32.load8_u offset=7 i32.const 34 i32.eq i32.and
    local.get $ptr i32.load8_u offset=8 i32.const 58 i32.eq i32.and
    local.get $ptr i32.load8_u offset=9 i32.const 34 i32.eq i32.and)

  (func $matches_order_id (param $ptr i32) (result i32)
    local.get $ptr i32.load8_u i32.const 34 i32.eq
    local.get $ptr i32.load8_u offset=1 i32.const 111 i32.eq i32.and
    local.get $ptr i32.load8_u offset=2 i32.const 114 i32.eq i32.and
    local.get $ptr i32.load8_u offset=3 i32.const 100 i32.eq i32.and
    local.get $ptr i32.load8_u offset=4 i32.const 101 i32.eq i32.and
    local.get $ptr i32.load8_u offset=5 i32.const 114 i32.eq i32.and
    local.get $ptr i32.load8_u offset=6 i32.const 73 i32.eq i32.and
    local.get $ptr i32.load8_u offset=7 i32.const 100 i32.eq i32.and
    local.get $ptr i32.load8_u offset=8 i32.const 34 i32.eq i32.and
    local.get $ptr i32.load8_u offset=9 i32.const 58 i32.eq i32.and
    local.get $ptr i32.load8_u offset=10 i32.const 34 i32.eq i32.and)

  (func $find_value
    (param $context_ptr i32) (param $context_len i32)
    (param $pattern i32) (param $pattern_len i32)
    (result i64)
    (local $scan i32) (local $start i32) (local $end i32) (local $matches i32)
    (block $found
      (loop $scan_value
        local.get $scan local.get $pattern_len i32.add local.get $context_len i32.gt_u
        if unreachable end
        local.get $pattern i32.eqz
        if (result i32)
          local.get $context_ptr local.get $scan i32.add
          call $matches_symbol
        else
          local.get $context_ptr local.get $scan i32.add
          call $matches_order_id
        end
        local.set $matches
        local.get $matches
        if
          local.get $context_ptr local.get $scan i32.add local.get $pattern_len i32.add
          local.set $start
          br $found
        end
        local.get $scan i32.const 1 i32.add local.set $scan
        br $scan_value))
    local.get $start local.set $end
    (block $end_found
      (loop $scan_end
        local.get $end local.get $context_ptr local.get $context_len i32.add i32.ge_u
        if unreachable end
        local.get $end i32.load8_u i32.const 34 i32.eq br_if $end_found
        local.get $end i32.const 1 i32.add local.set $end
        br $scan_end))
    local.get $start i64.extend_i32_u i64.const 32 i64.shl
    local.get $end local.get $start i32.sub i64.extend_i32_u i64.or)

  (func (export "run")
    (param $context_ptr i32) (param $context_len i32)
    (param $input_ptr i32) (param $input_len i32)
    (result i64)
    (local $symbol i64) (local $order_id i64)
    (local $source i32) (local $length i32) (local $copy i32) (local $dest i32)

    local.get $context_ptr local.get $context_len i32.const 0 i32.const 10 call $find_value
    local.set $symbol
    local.get $context_ptr local.get $context_len i32.const 1 i32.const 11 call $find_value
    local.set $order_id

    local.get $symbol i32.wrap_i64 local.tee $length i32.const 32 i32.gt_u
    if unreachable end
    local.get $symbol i64.const 32 i64.shr_u i32.wrap_i64 local.set $source
    i32.const 16425 local.set $dest
    (block $symbol_done
      (loop $copy_symbol
        local.get $copy local.get $length i32.ge_u br_if $symbol_done
        local.get $dest local.get $copy i32.add
        local.get $source local.get $copy i32.add i32.load8_u i32.store8
        local.get $copy i32.const 1 i32.add local.set $copy
        br $copy_symbol))

    local.get $dest local.get $length i32.add local.set $dest
    i32.const 0 local.set $copy
    (block $middle_done
      (loop $copy_middle
        local.get $copy i32.const 13 i32.ge_u br_if $middle_done
        local.get $dest local.get $copy i32.add
        i32.const 8192 local.get $copy i32.add i32.load8_u i32.store8
        local.get $copy i32.const 1 i32.add local.set $copy
        br $copy_middle))
    local.get $dest i32.const 13 i32.add local.set $dest

    local.get $order_id i32.wrap_i64 local.tee $length i32.const 128 i32.gt_u
    if unreachable end
    local.get $order_id i64.const 32 i64.shr_u i32.wrap_i64 local.set $source
    i32.const 0 local.set $copy
    (block $id_done
      (loop $copy_id
        local.get $copy local.get $length i32.ge_u br_if $id_done
        local.get $dest local.get $copy i32.add
        local.get $source local.get $copy i32.add i32.load8_u i32.store8
        local.get $copy i32.const 1 i32.add local.set $copy
        br $copy_id))
    local.get $dest local.get $length i32.add local.tee $dest i32.const 34 i32.store8
    local.get $dest i32.const 1 i32.add i32.const 125 i32.store8
    local.get $dest i32.const 2 i32.add i32.const 125 i32.store8

    i64.const 16384 i64.const 32 i64.shl
    local.get $dest i32.const 3 i32.add i32.const 16384 i32.sub
    i64.extend_i32_u i64.or))

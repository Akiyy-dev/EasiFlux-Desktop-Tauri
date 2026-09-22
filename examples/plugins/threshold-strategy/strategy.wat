(module
  (memory (export "memory") 1 16)

  ;; Strict compact-input tokens. The example deliberately rejects alternate
  ;; layouts instead of attempting to be a general JSON parser in Wasm.
  (data (i32.const 0) "{\"threshold\":\"")
  (data (i32.const 128) "\",\"order\":{\"symbol\":\"")
  (data (i32.const 256) "\",\"side\":\"")
  (data (i32.const 384) "\",\"orderType\":\"")
  (data (i32.const 512) "\",\"qty\":\"")
  (data (i32.const 640) "\",\"price\":\"")
  (data (i32.const 768) "\",\"timeInForce\":\"")
  (data (i32.const 896) "\",\"positionIdx\":")
  (data (i32.const 1024) ",\"reduceOnly\":")
  (data (i32.const 1152) "}}")
  (data (i32.const 1280) "Buy")
  (data (i32.const 1296) "Sell")
  (data (i32.const 1312) "Limit")
  (data (i32.const 1328) "GTC")
  (data (i32.const 1344) "IOC")
  (data (i32.const 1360) "FOK")
  (data (i32.const 1376) "true")
  (data (i32.const 1392) "false")
  (data (i32.const 1408) "{}")
  (data (i32.const 1424) "null")

  ;; Host-owned context tokens.
  (data (i32.const 2048) "\"state\":")
  (data (i32.const 2176) "{\"phase\":\"waiting\"}")
  (data (i32.const 2304) "{\"phase\":\"placed\"}")
  (data (i32.const 2432) "{\"phase\":\"awaitingOrder\",\"orderId\":\"")
  (data (i32.const 2560) "{\"phase\":\"cancelRequested\",\"orderId\":\"")
  (data (i32.const 2688) "{\"phase\":\"done\"}")
  (data (i32.const 2816) ",\"lastReceipt\":")
  (data (i32.const 2944) "\"},\"lastReceipt\":")
  (data (i32.const 3072) "\"kind\":\"placeOrder\",\"status\":\"accepted\"")
  (data (i32.const 3200) "\"orderId\":\"")
  (data (i32.const 3328) "\"market\":{\"ticker\":{\"symbol\":\"")
  (data (i32.const 3456) "\",\"lastPrice\":\"")
  (data (i32.const 3584) "\"orders\":{\"items\":[")
  (data (i32.const 3712) "\",\"status\":\"New\"")
  (data (i32.const 3840) "\",\"status\":\"PartiallyFilled\"")

  ;; Output fragments. Dynamic values are copied only after allowlist checks.
  (data (i32.const 4096) "{\"state\":{\"phase\":\"waiting\"},\"action\":{\"kind\":\"none\"},\"message\":\"Waiting for the configured threshold.\"}")
  (data (i32.const 4352) "{\"state\":{\"phase\":\"placed\"},\"action\":{\"kind\":\"placeOrder\",\"order\":")
  (data (i32.const 4480) "},\"message\":\"Threshold reached; requesting one limit order.\"}")
  (data (i32.const 4608) "{\"state\":{\"phase\":\"placed\"},\"action\":{\"kind\":\"none\"},\"message\":\"Waiting for the placement receipt.\"}")
  (data (i32.const 4864) "{\"state\":{\"phase\":\"awaitingOrder\",\"orderId\":\"")
  (data (i32.const 4992) "\"},\"action\":{\"kind\":\"none\"},\"message\":\"Placement accepted; waiting for the owned active order.\"}")
  (data (i32.const 5248) "\"},\"action\":{\"kind\":\"none\"},\"message\":\"Waiting for the owned active order to become visible.\"}")
  (data (i32.const 5504) "{\"state\":{\"phase\":\"cancelRequested\",\"orderId\":\"")
  (data (i32.const 5632) "\"},\"action\":{\"kind\":\"cancelOrder\",\"order\":{\"symbol\":\"")
  (data (i32.const 5760) "\",\"orderId\":\"")
  (data (i32.const 5888) "\"}},\"message\":\"Owned active order is visible; requesting one cancellation.\"}")
  (data (i32.const 6144) "{\"state\":{\"phase\":\"done\"},\"action\":{\"kind\":\"stop\"},\"message\":\"Cancellation was requested; stopping the example.\"}")
  (data (i32.const 6400) "{\"state\":{\"phase\":\"done\"},\"action\":{\"kind\":\"stop\"},\"message\":\"Placement was not accepted; stopping the example.\"}")

  (func (export "alloc") (param $bytes i32) (result i32)
    local.get $bytes
    i32.const 65535
    i32.add
    i32.const 16
    i32.shr_u
    memory.grow
    i32.const 65536
    i32.mul)

  (func $matches (param $ptr i32) (param $pattern i32) (param $length i32) (result i32)
    (local $i i32)
    (block $different
      (loop $next
        local.get $i local.get $length i32.ge_u br_if $different
        local.get $ptr local.get $i i32.add i32.load8_u
        local.get $pattern local.get $i i32.add i32.load8_u
        i32.ne
        if i32.const 0 return end
        local.get $i i32.const 1 i32.add local.set $i
        br $next))
    i32.const 1)

  (func $same (param $left i32) (param $left_len i32) (param $right i32) (param $right_len i32) (result i32)
    local.get $left_len local.get $right_len i32.ne
    if i32.const 0 return end
    local.get $left local.get $right local.get $left_len call $matches)

  (func $require (param $ptr i32) (param $pattern i32) (param $length i32)
    local.get $ptr local.get $pattern local.get $length call $matches
    i32.eqz
    if unreachable end)

  (func $find
    (param $ptr i32) (param $length i32) (param $pattern i32) (param $pattern_len i32)
    (result i32)
    (local $offset i32)
    local.get $pattern_len local.get $length i32.gt_u
    if i32.const 0 return end
    (block $missing
      (loop $scan
        local.get $offset local.get $pattern_len i32.add local.get $length i32.gt_u
        br_if $missing
        local.get $ptr local.get $offset i32.add
        local.get $pattern local.get $pattern_len call $matches
        if local.get $ptr local.get $offset i32.add return end
        local.get $offset i32.const 1 i32.add local.set $offset
        br $scan))
    i32.const 0)

  (func $find_byte (param $ptr i32) (param $end i32) (param $byte i32) (result i32)
    (loop $scan
      local.get $ptr local.get $end i32.ge_u
      if i32.const 0 return end
      local.get $ptr i32.load8_u local.get $byte i32.eq
      if local.get $ptr return end
      local.get $ptr i32.const 1 i32.add local.set $ptr
      br $scan)
    i32.const 0)

  ;; Return a packed pointer/length for an unescaped printable ASCII string.
  (func $read_string (param $ptr i32) (param $end i32) (param $maximum i32) (result i64)
    (local $scan i32) (local $byte i32)
    local.get $ptr local.set $scan
    (loop $next
      local.get $scan local.get $end i32.ge_u
      if unreachable end
      local.get $scan local.get $ptr i32.sub local.get $maximum i32.gt_u
      if unreachable end
      local.get $scan i32.load8_u local.tee $byte i32.const 34 i32.eq
      if
        local.get $scan local.get $ptr i32.eq
        if unreachable end
        local.get $ptr i64.extend_i32_u i64.const 32 i64.shl
        local.get $scan local.get $ptr i32.sub i64.extend_i32_u i64.or
        return
      end
      local.get $byte i32.const 32 i32.lt_u
      local.get $byte i32.const 127 i32.ge_u i32.or
      local.get $byte i32.const 92 i32.eq i32.or
      if unreachable end
      local.get $scan i32.const 1 i32.add local.set $scan
      br $next)
    unreachable)

  (func $valid_symbol (param $ptr i32) (param $length i32) (result i32)
    (local $i i32) (local $byte i32)
    local.get $length i32.const 1 i32.lt_u
    local.get $length i32.const 32 i32.gt_u i32.or
    if i32.const 0 return end
    (block $valid
      (loop $next
        local.get $i local.get $length i32.ge_u br_if $valid
        local.get $ptr local.get $i i32.add i32.load8_u local.tee $byte
        i32.const 65 i32.ge_u
        local.get $byte i32.const 90 i32.le_u i32.and
        local.get $byte i32.const 48 i32.ge_u
        local.get $byte i32.const 57 i32.le_u i32.and i32.or
        i32.eqz
        if i32.const 0 return end
        local.get $i i32.const 1 i32.add local.set $i
        br $next))
    i32.const 1)

  (func $valid_id (param $ptr i32) (param $length i32) (result i32)
    (local $i i32) (local $byte i32)
    local.get $length i32.const 1 i32.lt_u
    local.get $length i32.const 128 i32.gt_u i32.or
    if i32.const 0 return end
    (block $valid
      (loop $next
        local.get $i local.get $length i32.ge_u br_if $valid
        local.get $ptr local.get $i i32.add i32.load8_u local.tee $byte
        i32.const 65 i32.ge_u local.get $byte i32.const 90 i32.le_u i32.and
        local.get $byte i32.const 97 i32.ge_u local.get $byte i32.const 122 i32.le_u i32.and i32.or
        local.get $byte i32.const 48 i32.ge_u local.get $byte i32.const 57 i32.le_u i32.and i32.or
        local.get $byte i32.const 45 i32.eq i32.or
        local.get $byte i32.const 95 i32.eq i32.or
        i32.eqz
        if i32.const 0 return end
        local.get $i i32.const 1 i32.add local.set $i
        br $next))
    i32.const 1)

  ;; Canonical non-negative decimal, with optional positivity requirement.
  (func $valid_decimal (param $ptr i32) (param $length i32) (param $positive i32) (result i32)
    (local $i i32) (local $byte i32) (local $dot i32) (local $nonzero i32)
    local.get $length i32.eqz
    local.get $length i32.const 64 i32.gt_u i32.or
    if i32.const 0 return end
    local.get $ptr i32.load8_u i32.const 46 i32.eq
    local.get $ptr local.get $length i32.add i32.const 1 i32.sub i32.load8_u i32.const 46 i32.eq i32.or
    if i32.const 0 return end
    local.get $length i32.const 1 i32.gt_u
    local.get $ptr i32.load8_u i32.const 48 i32.eq i32.and
    local.get $ptr i32.load8_u offset=1 i32.const 46 i32.ne i32.and
    if i32.const 0 return end
    (block $valid
      (loop $next
        local.get $i local.get $length i32.ge_u br_if $valid
        local.get $ptr local.get $i i32.add i32.load8_u local.tee $byte
        i32.const 48 i32.ge_u
        local.get $byte i32.const 57 i32.le_u i32.and
        if
          local.get $byte i32.const 48 i32.ne
          if i32.const 1 local.set $nonzero end
        else
          local.get $byte i32.const 46 i32.ne local.get $dot i32.or
          if i32.const 0 return end
          i32.const 1 local.set $dot
        end
        local.get $i i32.const 1 i32.add local.set $i
        br $next))
    local.get $positive i32.eqz local.get $nonzero i32.or)

  (func $integer_length (param $ptr i32) (param $length i32) (result i32)
    (local $i i32)
    (loop $next
      local.get $i local.get $length i32.ge_u
      if local.get $length return end
      local.get $ptr local.get $i i32.add i32.load8_u i32.const 46 i32.eq
      if local.get $i return end
      local.get $i i32.const 1 i32.add local.set $i
      br $next)
    unreachable)

  ;; Inputs are canonical decimals. Return true when left >= right.
  (func $decimal_ge
    (param $left i32) (param $left_len i32) (param $right i32) (param $right_len i32)
    (result i32)
    (local $left_int i32) (local $right_int i32) (local $i i32)
    (local $left_byte i32) (local $right_byte i32) (local $fractions i32)
    local.get $left local.get $left_len call $integer_length local.set $left_int
    local.get $right local.get $right_len call $integer_length local.set $right_int
    local.get $left_int local.get $right_int i32.gt_u
    if i32.const 1 return end
    local.get $left_int local.get $right_int i32.lt_u
    if i32.const 0 return end
    (block $integer_equal
      (loop $integer
        local.get $i local.get $left_int i32.ge_u br_if $integer_equal
        local.get $left local.get $i i32.add i32.load8_u local.set $left_byte
        local.get $right local.get $i i32.add i32.load8_u local.set $right_byte
        local.get $left_byte local.get $right_byte i32.gt_u
        if i32.const 1 return end
        local.get $left_byte local.get $right_byte i32.lt_u
        if i32.const 0 return end
        local.get $i i32.const 1 i32.add local.set $i
        br $integer))
    i32.const 0 local.set $i
    local.get $left_len local.get $left_int i32.sub
    local.get $right_len local.get $right_int i32.sub
    local.get $left_len local.get $left_int i32.sub
    local.get $right_len local.get $right_int i32.sub
    i32.gt_u
    select
    local.set $fractions
    (block $equal
      (loop $fraction
        local.get $i local.get $fractions i32.ge_u br_if $equal
        i32.const 48 local.set $left_byte
        i32.const 48 local.set $right_byte
        local.get $left_int local.get $i i32.add i32.const 1 i32.add local.get $left_len i32.lt_u
        if local.get $left local.get $left_int i32.add local.get $i i32.add i32.const 1 i32.add i32.load8_u local.set $left_byte end
        local.get $right_int local.get $i i32.add i32.const 1 i32.add local.get $right_len i32.lt_u
        if local.get $right local.get $right_int i32.add local.get $i i32.add i32.const 1 i32.add i32.load8_u local.set $right_byte end
        local.get $left_byte local.get $right_byte i32.gt_u
        if i32.const 1 return end
        local.get $left_byte local.get $right_byte i32.lt_u
        if i32.const 0 return end
        local.get $i i32.const 1 i32.add local.set $i
        br $fraction))
    i32.const 1)

  (func $append (param $destination i32) (param $source i32) (param $length i32) (result i32)
    local.get $destination local.get $source local.get $length memory.copy
    local.get $destination local.get $length i32.add)

  (func $packed (param $ptr i32) (param $length i32) (result i64)
    local.get $ptr i64.extend_i32_u i64.const 32 i64.shl
    local.get $length i64.extend_i32_u i64.or)

  (func $awaiting_output (param $id i32) (param $id_len i32) (param $accepted i32) (result i64)
    (local $cursor i32)
    i32.const 24576 local.set $cursor
    local.get $cursor i32.const 4864 i32.const 45 call $append local.set $cursor
    local.get $cursor local.get $id local.get $id_len call $append local.set $cursor
    local.get $accepted
    if
      local.get $cursor i32.const 4992 i32.const 96 call $append local.set $cursor
    else
      local.get $cursor i32.const 5248 i32.const 94 call $append local.set $cursor
    end
    i32.const 24576 local.get $cursor i32.const 24576 i32.sub call $packed)

  (func $cancel_output
    (param $id i32) (param $id_len i32) (param $symbol i32) (param $symbol_len i32)
    (result i64)
    (local $cursor i32)
    i32.const 24576 local.set $cursor
    local.get $cursor i32.const 5504 i32.const 47 call $append local.set $cursor
    local.get $cursor local.get $id local.get $id_len call $append local.set $cursor
    local.get $cursor i32.const 5632 i32.const 53 call $append local.set $cursor
    local.get $cursor local.get $symbol local.get $symbol_len call $append local.set $cursor
    local.get $cursor i32.const 5760 i32.const 13 call $append local.set $cursor
    local.get $cursor local.get $id local.get $id_len call $append local.set $cursor
    local.get $cursor i32.const 5888 i32.const 76 call $append local.set $cursor
    i32.const 24576 local.get $cursor i32.const 24576 i32.sub call $packed)

  (func (export "run")
    (param $context_ptr i32) (param $context_len i32)
    (param $input_ptr i32) (param $input_len i32)
    (result i64)
    (local $context_end i32) (local $input_end i32) (local $cursor i32) (local $packed_value i64)
    (local $threshold i32) (local $threshold_len i32)
    (local $order_start i32) (local $order_len i32)
    (local $symbol i32) (local $symbol_len i32)
    (local $side i32) (local $side_len i32) (local $side_buy i32)
    (local $value i32) (local $value_len i32)
    (local $position i32) (local $reduce_only i32)
    (local $state i32) (local $state_value i32) (local $phase i32) (local $receipt i32)
    (local $owned_id i32) (local $owned_id_len i32)
    (local $market i32) (local $market_symbol i32) (local $market_symbol_len i32)
    (local $last_price i32) (local $last_price_len i32)
    (local $orders i32) (local $orders_end i32) (local $scan i32) (local $candidate i32)
    (local $candidate_id i32) (local $candidate_len i32) (local $object_end i32)
    (local $output i32)

    local.get $context_len i32.const 65536 i32.gt_u
    local.get $input_len i32.eqz i32.or
    local.get $input_len i32.const 4096 i32.gt_u i32.or
    if unreachable end
    local.get $context_ptr local.get $context_len i32.add local.set $context_end
    local.get $input_ptr local.get $input_len i32.add local.set $input_end

    ;; Parse the exact compact input object and validate every copied value.
    local.get $input_ptr i32.const 0 i32.const 14 call $require
    local.get $input_ptr i32.const 14 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 64 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $threshold_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $threshold
    local.get $threshold local.get $threshold_len i32.const 1 call $valid_decimal i32.eqz
    if unreachable end
    local.get $threshold local.get $threshold_len i32.add local.set $cursor

    local.get $cursor i32.const 128 i32.const 21 call $require
    local.get $cursor i32.const 10 i32.add local.set $order_start
    local.get $cursor i32.const 21 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 32 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $symbol_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $symbol
    local.get $symbol local.get $symbol_len call $valid_symbol i32.eqz
    if unreachable end
    local.get $symbol local.get $symbol_len i32.add local.set $cursor

    local.get $cursor i32.const 256 i32.const 10 call $require
    local.get $cursor i32.const 10 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 4 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $side_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $side
    local.get $side local.get $side_len i32.const 1280 i32.const 3 call $same local.tee $side_buy
    local.get $side local.get $side_len i32.const 1296 i32.const 4 call $same i32.or i32.eqz
    if unreachable end
    local.get $side local.get $side_len i32.add local.set $cursor

    local.get $cursor i32.const 384 i32.const 15 call $require
    local.get $cursor i32.const 15 i32.add local.set $cursor
    local.get $cursor i32.const 1312 i32.const 5 call $require
    local.get $cursor i32.const 5 i32.add i32.load8_u i32.const 34 i32.ne
    if unreachable end
    local.get $cursor i32.const 5 i32.add local.set $cursor

    local.get $cursor i32.const 512 i32.const 9 call $require
    local.get $cursor i32.const 9 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 64 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $value_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $value
    local.get $value local.get $value_len i32.const 1 call $valid_decimal i32.eqz
    if unreachable end
    local.get $value local.get $value_len i32.add local.set $cursor

    local.get $cursor i32.const 640 i32.const 11 call $require
    local.get $cursor i32.const 11 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 64 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $value_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $value
    local.get $value local.get $value_len i32.const 1 call $valid_decimal i32.eqz
    if unreachable end
    local.get $value local.get $value_len i32.add local.set $cursor

    local.get $cursor i32.const 768 i32.const 17 call $require
    local.get $cursor i32.const 17 i32.add local.set $cursor
    local.get $cursor local.get $input_end i32.const 3 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $value_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $value
    local.get $value local.get $value_len i32.const 1328 i32.const 3 call $same
    local.get $value local.get $value_len i32.const 1344 i32.const 3 call $same i32.or
    local.get $value local.get $value_len i32.const 1360 i32.const 3 call $same i32.or i32.eqz
    if unreachable end
    local.get $value local.get $value_len i32.add local.set $cursor

    local.get $cursor i32.const 896 i32.const 16 call $require
    local.get $cursor i32.const 16 i32.add local.set $cursor
    local.get $cursor i32.load8_u local.tee $position i32.const 49 i32.eq
    local.get $position i32.const 50 i32.eq i32.or i32.eqz
    if unreachable end
    local.get $position i32.const 48 i32.sub local.set $position
    local.get $cursor i32.const 1 i32.add local.set $cursor

    local.get $cursor i32.const 1024 i32.const 14 call $require
    local.get $cursor i32.const 14 i32.add local.set $cursor
    local.get $cursor i32.const 1376 i32.const 4 call $matches
    if
      i32.const 1 local.set $reduce_only
      local.get $cursor i32.const 4 i32.add local.set $cursor
    else
      local.get $cursor i32.const 1392 i32.const 5 call $require
      i32.const 0 local.set $reduce_only
      local.get $cursor i32.const 5 i32.add local.set $cursor
    end
    local.get $cursor local.set $value
    local.get $cursor i32.const 1152 i32.const 2 call $require
    local.get $cursor i32.const 2 i32.add local.get $input_end i32.ne
    if unreachable end
    local.get $value i32.const 1 i32.add local.get $order_start i32.sub local.set $order_len

    ;; positionIdx must agree with side and reduceOnly.
    local.get $side_buy local.get $reduce_only i32.ne
    if (result i32) i32.const 1 else i32.const 2 end
    local.get $position i32.ne
    if unreachable end

    ;; Read the selected market price and prove it belongs to the order symbol.
    local.get $context_ptr local.get $context_len i32.const 3328 i32.const 30 call $find local.tee $market i32.eqz
    if unreachable end
    local.get $market i32.const 30 i32.add local.set $cursor
    local.get $cursor local.get $context_end i32.const 32 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $market_symbol_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $market_symbol
    local.get $market_symbol local.get $market_symbol_len local.get $symbol local.get $symbol_len call $same i32.eqz
    if unreachable end
    local.get $market_symbol local.get $market_symbol_len i32.add local.set $cursor
    local.get $cursor i32.const 3456 i32.const 15 call $require
    local.get $cursor i32.const 15 i32.add local.set $cursor
    local.get $cursor local.get $context_end i32.const 64 call $read_string local.set $packed_value
    local.get $packed_value i32.wrap_i64 local.set $last_price_len
    local.get $packed_value i64.const 32 i64.shr_u i32.wrap_i64 local.set $last_price
    local.get $last_price local.get $last_price_len i32.const 0 call $valid_decimal i32.eqz
    if unreachable end

    ;; Parse only states emitted by this guest.
    local.get $context_ptr local.get $context_len i32.const 2048 i32.const 8 call $find local.tee $state i32.eqz
    if unreachable end
    local.get $state i32.const 8 i32.add local.set $state_value
    local.get $state_value i32.const 1408 i32.const 2 call $matches
    if
      i32.const 0 local.set $phase
      local.get $state_value i32.const 2 i32.add local.set $cursor
      local.get $cursor i32.const 2816 i32.const 15 call $require
      local.get $cursor i32.const 15 i32.add local.set $receipt
    else
      local.get $state_value i32.const 2176 i32.const 19 call $matches
      if
        i32.const 0 local.set $phase
        local.get $state_value i32.const 19 i32.add local.set $cursor
        local.get $cursor i32.const 2816 i32.const 15 call $require
        local.get $cursor i32.const 15 i32.add local.set $receipt
      else
        local.get $state_value i32.const 2304 i32.const 18 call $matches
        if
          i32.const 1 local.set $phase
          local.get $state_value i32.const 18 i32.add local.set $cursor
          local.get $cursor i32.const 2816 i32.const 15 call $require
          local.get $cursor i32.const 15 i32.add local.set $receipt
        else
          local.get $state_value i32.const 2432 i32.const 36 call $matches
          if
            i32.const 2 local.set $phase
            local.get $state_value i32.const 36 i32.add local.set $owned_id
          else
            local.get $state_value i32.const 2560 i32.const 38 call $matches
            if
              i32.const 3 local.set $phase
              local.get $state_value i32.const 38 i32.add local.set $owned_id
            else
              local.get $state_value i32.const 2688 i32.const 16 call $matches
              if
                i32.const 4 local.set $phase
                local.get $state_value i32.const 16 i32.add local.set $cursor
                local.get $cursor i32.const 2816 i32.const 15 call $require
                local.get $cursor i32.const 15 i32.add local.set $receipt
              else
                unreachable
              end
            end
          end
          local.get $phase i32.const 2 i32.eq local.get $phase i32.const 3 i32.eq i32.or
          if
            local.get $owned_id local.get $context_end i32.const 128 call $read_string local.set $packed_value
            local.get $packed_value i32.wrap_i64 local.set $owned_id_len
            local.get $owned_id local.get $owned_id_len call $valid_id i32.eqz
            if unreachable end
            local.get $owned_id local.get $owned_id_len i32.add local.set $cursor
            local.get $cursor i32.const 2944 i32.const 17 call $require
            local.get $cursor i32.const 17 i32.add local.set $receipt
          end
        end
      end
    end

    ;; Initial/waiting: threshold controls the one placement request.
    local.get $phase i32.eqz
    if
      local.get $last_price local.get $last_price_len local.get $threshold local.get $threshold_len call $decimal_ge
      i32.eqz
      if i32.const 4096 i32.const 104 call $packed return end
      i32.const 24576 local.set $output
      local.get $output i32.const 4352 i32.const 66 call $append local.set $cursor
      local.get $cursor local.get $order_start local.get $order_len call $append local.set $cursor
      local.get $cursor i32.const 4480 i32.const 61 call $append local.set $cursor
      i32.const 24576 local.get $cursor i32.const 24576 i32.sub call $packed return
    end

    ;; Placed: do not place again. Only an accepted native receipt supplies ownership.
    local.get $phase i32.const 1 i32.eq
    if
      local.get $receipt i32.const 1424 i32.const 4 call $matches
      if i32.const 4608 i32.const 100 call $packed return end
      local.get $receipt local.get $context_end local.get $receipt i32.sub
      i32.const 3072 i32.const 39 call $find i32.eqz
      if i32.const 6400 i32.const 113 call $packed return end
      local.get $receipt local.get $context_end local.get $receipt i32.sub
      i32.const 3200 i32.const 11 call $find local.tee $candidate i32.eqz
      if unreachable end
      local.get $candidate i32.const 11 i32.add local.set $owned_id
      local.get $owned_id local.get $context_end i32.const 128 call $read_string local.set $packed_value
      local.get $packed_value i32.wrap_i64 local.set $owned_id_len
      local.get $owned_id local.get $owned_id_len call $valid_id i32.eqz
      if unreachable end
      local.get $owned_id local.get $owned_id_len i32.const 1 call $awaiting_output return
    end

    ;; Awaiting: cancel only the exact receipt-derived ID when it is active in orders.
    local.get $phase i32.const 2 i32.eq
    if
      local.get $context_ptr local.get $context_len i32.const 3584 i32.const 19 call $find local.tee $orders i32.eqz
      if local.get $owned_id local.get $owned_id_len i32.const 0 call $awaiting_output return end
      local.get $market local.set $orders_end
      local.get $orders local.get $orders_end i32.ge_u
      if unreachable end
      local.get $orders local.set $scan
      (block $not_visible
        (loop $next_order
          local.get $scan local.get $orders_end local.get $scan i32.sub
          i32.const 3200 i32.const 11 call $find local.tee $candidate i32.eqz br_if $not_visible
          local.get $candidate i32.const 11 i32.add local.set $candidate_id
          local.get $candidate_id local.get $orders_end i32.const 128 call $read_string local.set $packed_value
          local.get $packed_value i32.wrap_i64 local.set $candidate_len
          local.get $candidate_id local.get $candidate_len local.get $owned_id local.get $owned_id_len call $same
          if
            local.get $candidate_id local.get $orders_end i32.const 125 call $find_byte local.tee $object_end i32.eqz
            if unreachable end
            local.get $candidate local.get $object_end local.get $candidate i32.sub
            i32.const 3712 i32.const 16 call $find
            local.get $candidate local.get $object_end local.get $candidate i32.sub
            i32.const 3840 i32.const 27 call $find i32.or
            if
              local.get $owned_id local.get $owned_id_len local.get $symbol local.get $symbol_len call $cancel_output return
            end
            br $not_visible
          end
          local.get $candidate_id local.get $candidate_len i32.add i32.const 1 i32.add local.set $scan
          br $next_order))
      local.get $owned_id local.get $owned_id_len i32.const 0 call $awaiting_output return
    end

    ;; Once a cancellation was requested, never request it twice.
    local.get $phase i32.const 3 i32.eq local.get $phase i32.const 4 i32.eq i32.or
    if i32.const 6144 i32.const 113 call $packed return end
    unreachable))

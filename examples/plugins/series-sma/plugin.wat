(module
  ;; The host accepts at most 4096 f64 values (32768 bytes). One 64 KiB
  ;; page is enough for the input and the explicit 16-page maximum matches
  ;; the host's 1 MiB linear-memory limit.
  (memory (export "memory") 1 16)

  ;; A fresh instance is created for every run, so offset zero is available.
  (func (export "alloc") (param $bytes i32) (result i32)
    i32.const 0)

  (func (export "run")
    (param $ptr i32)
    (param $count i32)
    (param $period f64)
    (result f64)
    (local $period_i32 i32)
    (local $index i32)
    (local $sum f64)

    ;; Fractional periods and periods outside the supplied series are invalid.
    ;; The host rejects this non-finite result with plugin_compute_invalid_output.
    local.get $period
    f64.const 1
    f64.lt
    if
      f64.const nan
      return
    end

    local.get $period
    local.get $count
    f64.convert_i32_u
    f64.gt
    if
      f64.const nan
      return
    end

    local.get $period
    i32.trunc_f64_s
    local.tee $period_i32
    f64.convert_i32_s
    local.get $period
    f64.ne
    if
      f64.const nan
      return
    end

    local.get $count
    local.get $period_i32
    i32.sub
    local.set $index

    (loop $sum_next
      local.get $sum
      local.get $ptr
      local.get $index
      i32.const 8
      i32.mul
      i32.add
      f64.load
      f64.add
      local.set $sum

      local.get $index
      i32.const 1
      i32.add
      local.tee $index
      local.get $count
      i32.lt_u
      br_if $sum_next)

    local.get $sum
    local.get $period
    f64.div))

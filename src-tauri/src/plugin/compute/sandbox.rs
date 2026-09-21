use super::*;
use wasmparser::{Parser, Payload};

const MEMORY_BYTES: usize = 1024 * 1024;
const TOTAL_FUEL: u64 = 2_000_000;
const FUEL_SLICE: u64 = 10_000;
const DEADLINE: Duration = Duration::from_secs(2);

/// Parsing counts before iterating prevents tiny binaries with huge declarations
/// from causing unbounded validation/translation work. Wasmi still validates all
/// Wasm semantics; this pass is strictly a resource and authority allowlist.
fn preflight(bytes: &[u8]) -> AppResult<()> {
    let invalid = || error("plugin_compute_invalid_module");
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|_| invalid())? {
            Payload::Version { .. } | Payload::End(_) | Payload::CustomSection(_) => {}
            Payload::TypeSection(reader) => {
                if reader.count() > 128 {
                    return Err(invalid());
                }
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(|_| invalid())?;
                    if ty.params().len() > 16 || ty.results().len() > 1 {
                        return Err(invalid());
                    }
                }
            }
            Payload::ImportSection(reader) if reader.count() == 0 => {}
            Payload::FunctionSection(reader) if reader.count() <= 128 => {}
            Payload::GlobalSection(reader) if reader.count() <= 64 => {}
            Payload::ExportSection(reader) if reader.count() <= 32 => {}
            Payload::DataSection(reader) if reader.count() <= 32 => {}
            Payload::DataCountSection { count, .. } if count <= 32 => {}
            Payload::MemorySection(reader) => {
                if reader.count() != 1 {
                    return Err(invalid());
                }
                for memory in reader {
                    let memory = memory.map_err(|_| invalid())?;
                    if memory.memory64
                        || memory.shared
                        || memory.page_size_log2.is_some()
                        || memory.initial > 16
                        || memory.maximum.is_some_and(|max| max > 16)
                    {
                        return Err(invalid());
                    }
                }
            }
            Payload::CodeSectionStart { count, .. } if count <= 128 => {}
            Payload::CodeSectionEntry(body) => {
                let reader = body.get_locals_reader().map_err(|_| invalid())?;
                if reader.get_count() > 256 {
                    return Err(invalid());
                }
                let mut locals = 0u32;
                for local in reader {
                    locals = locals
                        .checked_add(local.map_err(|_| invalid())?.0)
                        .ok_or_else(invalid)?;
                    if locals > 256 {
                        return Err(invalid());
                    }
                }
            }
            // Includes all imports, start, tables/elements, tags, unknown sections
            // and components. No linker function is ever defined.
            _ => return Err(invalid()),
        }
    }
    Ok(())
}

fn engine() -> Engine {
    let mut config = Config::default();
    config
        .consume_fuel(true)
        .allow_start_fn(false)
        .ignore_custom_sections(true)
        .compilation_mode(CompilationMode::Eager)
        .enforced_limits(EnforcedLimits::strict())
        .set_min_stack_height(1024)
        .set_max_stack_height(64 * 1024)
        .set_max_recursion_depth(64)
        .set_max_cached_stacks(0)
        .wasm_mutable_global(false)
        .wasm_sign_extension(false)
        .wasm_saturating_float_to_int(false)
        .wasm_multi_value(false)
        .wasm_multi_memory(false)
        .wasm_bulk_memory(false)
        .wasm_reference_types(false)
        .wasm_tail_call(false)
        .wasm_extended_const(false)
        .wasm_custom_page_sizes(false)
        .wasm_wide_arithmetic(false);
    Engine::new(&config)
}

struct Budget<'a> {
    cancel: &'a AtomicBool,
    started: Instant,
    unissued_fuel: u64,
}

impl Budget<'_> {
    fn check(&self) -> AppResult<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(error("plugin_compute_cancelled"));
        }
        if self.started.elapsed() >= DEADLINE {
            return Err(error("plugin_compute_deadline"));
        }
        Ok(())
    }

    fn call<P: WasmParams, R: WasmResults>(
        &mut self,
        store: &mut Store<StoreLimits>,
        func: TypedFunc<P, R>,
        params: P,
    ) -> AppResult<R> {
        self.check()?;
        let call = func
            .call_resumable(&mut *store, params)
            .map_err(|_| error("plugin_compute_trap"))?;
        self.finish_call(store, call)
    }

    fn finish_call<R: WasmResults>(
        &mut self,
        store: &mut Store<StoreLimits>,
        mut call: TypedResumableCall<R>,
    ) -> AppResult<R> {
        loop {
            self.check()?;
            match call {
                TypedResumableCall::Finished(value) => return Ok(value),
                TypedResumableCall::HostTrap(_) => return Err(error("plugin_compute_trap")),
                TypedResumableCall::OutOfFuel(suspended) => {
                    let remaining = store
                        .get_fuel()
                        .map_err(|_| error("plugin_compute_internal"))?;
                    // Never accumulate more than one slice. A single instruction
                    // too expensive for a slice fails closed rather than spinning.
                    let added = (FUEL_SLICE - remaining).min(self.unissued_fuel);
                    if added == 0 || suspended.required_fuel() > FUEL_SLICE {
                        return Err(error("plugin_compute_budget"));
                    }
                    self.unissued_fuel -= added;
                    store
                        .set_fuel(remaining + added)
                        .map_err(|_| error("plugin_compute_internal"))?;
                    call = suspended
                        .resume(&mut *store)
                        .map_err(|_| error("plugin_compute_trap"))?;
                }
            }
        }
    }
}

pub(crate) fn validate_input(
    values: &[f64],
    parameter: f64,
    params: &PluginComputeParams,
) -> AppResult<()> {
    if !(1..=4096).contains(&values.len()) || values.iter().any(|value| !value.is_finite()) {
        return Err(error("plugin_compute_invalid_input"));
    }
    if !parameter.is_finite()
        || parameter < f64::from(params.parameter.min)
        || parameter > f64::from(params.parameter.max)
    {
        return Err(error("plugin_compute_invalid_parameter"));
    }
    Ok(())
}

pub(crate) fn execute_guest(
    params: &PluginComputeParams,
    values: &[f64],
    parameter: f64,
    cancel: &AtomicBool,
) -> AppResult<f64> {
    let mut budget = Budget {
        cancel,
        started: Instant::now(),
        unissued_fuel: TOTAL_FUEL - FUEL_SLICE,
    };
    budget.check()?;
    validate_input(values, parameter, params)?;
    params
        .validate()
        .map_err(|_| error("plugin_compute_invalid_module"))?;
    let bytes = params
        .module_bytes()
        .map_err(|_| error("plugin_compute_invalid_module"))?;
    let (mut store, instance) = instantiate(&bytes, &budget)?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| error("plugin_compute_invalid_abi"))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "alloc")
        .map_err(|_| error("plugin_compute_invalid_abi"))?;
    let run = instance
        .get_typed_func::<(i32, i32, f64), f64>(&store, "run")
        .map_err(|_| error("plugin_compute_invalid_abi"))?;
    let length = values.len() * 8;
    let ptr = budget.call(&mut store, alloc, length as i32)?;
    let offset = usize::try_from(ptr).map_err(|_| error("plugin_compute_memory"))?;
    checked_range(offset, length, memory.data_size(&store))?;
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    memory
        .write(&mut store, offset, &bytes)
        .map_err(|_| error("plugin_compute_memory"))?;
    let value = budget.call(&mut store, run, (ptr, values.len() as i32, parameter))?;
    if !value.is_finite() {
        return Err(error("plugin_compute_invalid_output"));
    }
    Ok(value)
}

/// Single allowlist/configuration/budget setup for both guest ABIs.
fn instantiate(
    bytes: &[u8],
    budget: &Budget<'_>,
) -> AppResult<(Store<StoreLimits>, wasmi::Instance)> {
    preflight(bytes)?;
    let engine = engine();
    let module = Module::new(&engine, bytes).map_err(|_| error("plugin_compute_invalid_module"))?;
    if module.imports().next().is_some() {
        return Err(error("plugin_compute_invalid_module"));
    }
    budget.check()?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(MEMORY_BYTES)
        .memories(1)
        .tables(0)
        .table_elements(0)
        .instances(1)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, limits);
    store.limiter(|limits| limits);
    store
        .set_fuel(FUEL_SLICE)
        .map_err(|_| error("plugin_compute_internal"))?;
    // Start has already been rejected both by preflight and Config.
    let instance = Linker::<StoreLimits>::new(&engine)
        .instantiate_and_start(&mut store, &module)
        .map_err(|_| error("plugin_compute_invalid_module"))?;
    Ok((store, instance))
}

fn checked_range(offset: usize, len: usize, size: usize) -> AppResult<()> {
    if offset.checked_add(len).is_none_or(|end| end > size) {
        return Err(error("plugin_compute_memory"));
    }
    Ok(())
}

pub(crate) fn execute_json(
    bytes: &[u8],
    context: &[u8],
    input: &[u8],
    cancel: &AtomicBool,
) -> AppResult<Vec<u8>> {
    if context.len() > 65_536 || input.len() > 4096 {
        return Err(error("plugin_compute_invalid_input"));
    }
    let mut budget = Budget {
        cancel,
        started: Instant::now(),
        unissued_fuel: TOTAL_FUEL - FUEL_SLICE,
    };
    budget.check()?;
    let (mut store, instance) = instantiate(bytes, &budget)?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| error("plugin_compute_invalid_abi"))?;
    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "alloc")
        .map_err(|_| error("plugin_compute_invalid_abi"))?;
    let run = instance
        .get_typed_func::<(i32, i32, i32, i32), i64>(&store, "run")
        .map_err(|_| error("plugin_compute_invalid_abi"))?;
    let cp = budget.call(&mut store, alloc, context.len() as i32)?;
    let ip = budget.call(&mut store, alloc, input.len() as i32)?;
    let co = usize::try_from(cp).map_err(|_| error("plugin_compute_memory"))?;
    let io = usize::try_from(ip).map_err(|_| error("plugin_compute_memory"))?;
    checked_range(co, context.len(), memory.data_size(&store))?;
    checked_range(io, input.len(), memory.data_size(&store))?;
    if co < io + input.len() && io < co + context.len() {
        return Err(error("plugin_compute_memory"));
    }
    memory
        .write(&mut store, co, context)
        .map_err(|_| error("plugin_compute_memory"))?;
    memory
        .write(&mut store, io, input)
        .map_err(|_| error("plugin_compute_memory"))?;
    let packed = budget.call(
        &mut store,
        run,
        (cp, context.len() as i32, ip, input.len() as i32),
    )? as u64;
    let offset = (packed >> 32) as usize;
    let len = (packed & 0xffff_ffff) as usize;
    if len > 16_384 {
        return Err(error("plugin_compute_invalid_output"));
    }
    checked_range(offset, len, memory.data_size(&store))?;
    let output = memory.data(&store)[offset..offset + len].to_vec();
    std::str::from_utf8(&output).map_err(|_| error("plugin_compute_invalid_output"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_deadline_and_cancel_are_checked_at_each_boundary() {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            started: Instant::now() - Duration::from_secs(3),
            unissued_fuel: TOTAL_FUEL,
        };
        let code = |result: AppResult<()>| {
            serde_json::to_value(result.unwrap_err()).unwrap()["code"].clone()
        };
        assert_eq!(code(budget.check()), "plugin_compute_deadline");
        budget.started = Instant::now();
        assert!(budget.check().is_ok());
        cancel.store(true, Ordering::Release);
        assert_eq!(code(budget.check()), "plugin_compute_cancelled");
    }

    #[test]
    fn running_infinite_guest_cancels_between_actual_fuel_slices() {
        let engine = engine();
        let bytes = wat::parse_str(
            r#"(module (func (export "loop") (result f64) (loop $again br $again) f64.const 0))"#,
        )
        .unwrap();
        let module = Module::new(&engine, bytes).unwrap();
        let mut store = Store::new(&engine, StoreLimitsBuilder::new().build());
        store.set_fuel(FUEL_SLICE).unwrap();
        let instance = Linker::new(&engine)
            .instantiate_and_start(&mut store, &module)
            .unwrap();
        let func = instance.get_typed_func::<(), f64>(&store, "loop").unwrap();
        let suspended = func.call_resumable(&mut store, ()).unwrap();
        assert!(matches!(suspended, TypedResumableCall::OutOfFuel(_)));
        let cancel = AtomicBool::new(true);
        let mut budget = Budget {
            cancel: &cancel,
            started: Instant::now(),
            unissued_fuel: TOTAL_FUEL - FUEL_SLICE,
        };
        assert_eq!(
            serde_json::to_value(budget.finish_call(&mut store, suspended).unwrap_err()).unwrap()
                ["code"],
            "plugin_compute_cancelled"
        );
    }
}

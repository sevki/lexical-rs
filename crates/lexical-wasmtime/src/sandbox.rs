//! The wasmtime sandbox shared by every kind of plugin: engine, store, limits and the
//! deliberately empty WASI context.

use lexical_plugin_host::{PluginError, Result};
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

/// What one call into a plugin may consume before it is stopped.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    /// Wasm instructions (roughly) per call.
    pub fuel: u64,
    /// Linear memory the plugin may grow to.
    pub memory_bytes: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { fuel: 50_000_000, memory_bytes: 64 << 20 }
    }
}

/// Fuel for instantiating a plugin and asking it for its `info`.
pub(crate) const LOAD_FUEL: u64 = 500_000_000;

pub(crate) struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

/// A compiled component with a store ready to instantiate it in.
pub(crate) struct Sandbox {
    pub component: Component,
    pub linker: Linker<HostState>,
    pub store: Store<HostState>,
}

pub(crate) fn load_error(e: wasmtime::Error) -> PluginError {
    PluginError::Load(format!("{e:#}"))
}

pub(crate) fn call_error(e: wasmtime::Error) -> PluginError {
    PluginError::Call(format!("{e:#}"))
}

/// Compile `bytes` and prepare a store limited by `budget`.
pub(crate) fn prepare(bytes: &[u8], budget: Budget) -> Result<Sandbox> {
    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).map_err(load_error)?;
    let component = Component::new(&engine, bytes).map_err(load_error)?;

    // WASI is linked because Rust's (and JavaScript's) runtimes import it; the guest is
    // given an empty context: no files, no environment, no network, no inherited stdio.
    let mut linker = Linker::<HostState>::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(load_error)?;

    let state = HostState {
        wasi: WasiCtx::builder().build(),
        table: ResourceTable::new(),
        // A component is several core instances (the guest, adapters, shims).
        limits: StoreLimitsBuilder::new().memory_size(budget.memory_bytes).instances(32).build(),
    };
    let mut store = Store::new(&engine, state);
    store.limiter(|s| &mut s.limits);
    // Starting the plugin gets a fixed allowance; the budget applies to each call.
    store.set_fuel(LOAD_FUEL).map_err(load_error)?;
    Ok(Sandbox { component, linker, store })
}

/// Give the next call its fuel.
pub(crate) fn arm(store: &mut Store<HostState>, budget: Budget) -> Result<()> {
    store.set_fuel(budget.fuel).map_err(|e| PluginError::Call(e.to_string()))
}

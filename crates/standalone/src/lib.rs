use sies_chain::rebuilder::Rebuilder;
use sies_core::engine::{SwarmEvolutionConfig, SwarmEvolutionEngine};
use sies_storage::FileLedger;

pub mod config;
pub mod error;

pub use config::StandaloneConfig;
pub use error::StandaloneError;

/// Standalone 运行时适配器
#[allow(dead_code)]
pub struct StandaloneRuntime {
    ledger: FileLedger,
    engine: SwarmEvolutionEngine,
}

impl StandaloneRuntime {
    pub fn start(config: &StandaloneConfig) -> Result<Self, StandaloneError> {
        let ledger = FileLedger::open(&config.data_dir)?;

        // Rebuilder 接受实现了 Ledger 的类型，FileLedger 本身已实现
        let rebuilder = Rebuilder::new(&ledger);
        let _state_store = rebuilder.rebuild()?;

        let engine_config = SwarmEvolutionConfig::default();
        let engine = SwarmEvolutionEngine::new(engine_config);

        tracing::info!(
            data_dir = %config.data_dir.display(),
            "Standalone runtime initialized"
        );

        Ok(Self { ledger, engine })
    }

    /// 获取当前状态根哈希
    #[must_use]
    pub fn current_root(&self) -> [u8; 32] {
        // TODO: 从 engine 或 state_store 中获取实际 root hash
        [0u8; 32]
    }
}

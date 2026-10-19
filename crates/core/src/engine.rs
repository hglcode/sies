use crate::bayesian::{BayesianError, BayesianUpdater, BetaState};
use crate::belief::Belief;
use crate::scoring::ScoringRule;
use crate::swarm::{SwarmConfig, SwarmUpdater};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 群体智能演化引擎配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmEvolutionConfig {
    pub bayesian_learning_rate: Decimal,
    pub swarm: SwarmConfig,
}

impl Default for SwarmEvolutionConfig {
    fn default() -> Self {
        Self {
            bayesian_learning_rate: Decimal::new(1, 1), // 0.1
            swarm: SwarmConfig::default(),
        }
    }
}

/// 编排贝叶斯个体学习 + 群体智能演化的核心引擎
pub struct SwarmEvolutionEngine {
    bayesian: BayesianUpdater,
    swarm: SwarmUpdater,
}

impl SwarmEvolutionEngine {
    #[must_use]
    pub fn new(config: SwarmEvolutionConfig) -> Self {
        Self {
            bayesian: BayesianUpdater::new(ScoringRule::Brier, config.bayesian_learning_rate),
            swarm: SwarmUpdater::new(config.swarm),
        }
    }

    /// 完整的一轮群体演化：
    /// 1. 每个 agent 独立做贝叶斯更新 (个体认知)
    /// 2. 全局群体信息素演化 (群体涌现)
    ///
    /// # Arguments
    /// * `states` - 各 agent 当前 `BetaState`
    /// * `beliefs` - 各 agent 本轮预测 `Belief`
    /// * `outcome` - 本轮真实结果
    pub fn step(
        &self,
        states: &[BetaState],
        beliefs: &[Belief],
        outcome: &Belief,
    ) -> Result<Vec<BetaState>, BayesianError> {
        // Phase 1: 逐 agent 贝叶斯更新
        let bayesian_updated: Vec<BetaState> = states
            .iter()
            .zip(beliefs.iter())
            .map(|(state, belief)| self.bayesian.update(state, belief, outcome))
            .collect::<Result<Vec<_>, _>>()?;

        // Phase 2: 群体智能演化
        self.swarm.evolve(&bayesian_updated, beliefs, outcome)
    }
}

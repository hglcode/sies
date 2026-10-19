use crate::bayesian::{BayesianError, BetaState};
use crate::belief::Belief;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 群体智能演化配置 (当前默认实现为蚁群信息素机制)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmConfig {
    /// 信息素蒸发率 ρ ∈ (0, 1)
    pub evaporation_rate: Decimal,
    /// 信息素沉积强度 Q
    pub deposit_strength: Decimal,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            evaporation_rate: Decimal::new(5, 2), // 0.05
            deposit_strength: Decimal::ONE,
        }
    }
}

pub struct SwarmUpdater {
    config: SwarmConfig,
}

impl SwarmUpdater {
    #[must_use]
    pub fn new(config: SwarmConfig) -> Self {
        Self { config }
    }

    /// 对一组 `BetaState` 执行群体层面的信息素蒸发 + 基于 outcome 的沉积
    ///
    /// # 算法逻辑
    /// 1. 所有 agent 的权重先经历全局蒸发: W' = W * (1 - ρ)
    /// 2. 预测准确的 agent 获得额外信息素沉积: W'' = W' + Q * score
    /// 3. 将新的权重反解回 `BetaState` 的 (alpha, beta)
    pub fn evolve(
        &self,
        states: &[BetaState],
        beliefs: &[Belief],
        outcome: &Belief,
    ) -> Result<Vec<BetaState>, BayesianError> {
        let one_minus_rho = Decimal::ONE - self.config.evaporation_rate;

        states
            .iter()
            .zip(beliefs.iter()) // ← zip beliefs
            .map(|(state, belief)| {
                // Phase 1: 全局蒸发
                let old_weight = state.expected_weight();
                let evaporated_weight = old_weight * one_minus_rho;

                // Phase 2: 基于 belief vs outcome 的信息素沉积
                // 计算 belief 与 outcome 的点积作为准确度
                let accuracy: Decimal = belief
                    .allocations()
                    .iter()
                    .zip(outcome.allocations().iter())
                    .map(|(b, o)| b * o)
                    .sum();

                let deposit = self.config.deposit_strength * accuracy;
                let new_weight = (evaporated_weight + deposit)
                    .min(Decimal::ONE)
                    .max(Decimal::new(1, 4));

                // Phase 3: 将权重映射回 BetaState
                let total = state.alpha() + state.beta();
                let new_alpha = new_weight * total;
                let new_beta = (Decimal::ONE - new_weight) * total;

                BetaState::new(
                    new_alpha.max(Decimal::new(1, 4)),
                    new_beta.max(Decimal::new(1, 4)),
                )
            })
            .collect()
    }
}

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
        // P0-2 整改: Phase 1 的 zip 同样静默截断，先显式校验长度
        // （旧实现: 3 states + 2 beliefs → Phase 1 截成 2，Phase 2 再截，静默返回 2）
        if states.len() != beliefs.len() {
            return Err(BayesianError::LengthMismatch {
                states_len: states.len(),
                beliefs_len: beliefs.len(),
            });
        }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    fn belief(vals: &[f64]) -> Belief {
        Belief::new(
            vals.iter()
                .map(|&v| Decimal::from_f64(v).unwrap())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn step_rejects_length_mismatch() {
        let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
        let states = vec![BetaState::uniform_prior(); 3];
        let beliefs = vec![belief(&[0.5, 0.5]); 2];
        let outcome = belief(&[1.0, 0.0]);
        assert_eq!(
            engine.step(&states, &beliefs, &outcome),
            Err(BayesianError::LengthMismatch {
                states_len: 3,
                beliefs_len: 2
            })
        );
    }

    #[test]
    fn step_equal_lengths_returns_same_count() {
        let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
        let states = vec![BetaState::uniform_prior(); 5];
        let beliefs = vec![belief(&[0.8, 0.2]); 5];
        let outcome = belief(&[1.0, 0.0]);
        let out = engine.step(&states, &beliefs, &outcome).unwrap();
        assert_eq!(out.len(), 5);
    }
}

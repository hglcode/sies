use crate::bayesian::{BayesianError, BetaState};
use crate::belief::Belief;
use crate::scoring::ScoringError;

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
    ///
    /// # Errors
    /// * [`BayesianError::LengthMismatch`] - states 与 beliefs 数量不等
    ///   （P0-2 整改：旧实现 zip 静默截断至较短一侧，多出的 agent 被无声丢弃）
    /// * [`BayesianError::Scoring`] - belief 与 outcome 选项维度失配
    ///   （防御纵深：内层点积 zip 同样会截断）
    pub fn evolve(
        &self,
        states: &[BetaState],
        beliefs: &[Belief],
        outcome: &Belief,
    ) -> Result<Vec<BetaState>, BayesianError> {
        // P0-2 整改: 长度失配显式报错
        if states.len() != beliefs.len() {
            return Err(BayesianError::LengthMismatch {
                states_len: states.len(),
                beliefs_len: beliefs.len(),
            });
        }
        // 防御纵深: belief 与 outcome 选项维度失配时，点积 zip 同样截断
        if beliefs
            .iter()
            .any(|b| b.num_options() != outcome.num_options())
        {
            return Err(BayesianError::Scoring(ScoringError::DimensionMismatch));
        }

        let one_minus_rho = Decimal::ONE - self.config.evaporation_rate;

        states
            .iter()
            .zip(beliefs.iter()) // 长度已在入口校验相等，zip 安全
            .map(|(state, belief)| {
                // Phase 1: 全局蒸发
                let old_weight = state.expected_weight();
                let evaporated_weight = old_weight * one_minus_rho;

                // Phase 2: 基于 belief vs outcome 的信息素沉积
                // 计算 belief 与 outcome 的点积作为准确度（维度已校验，zip 安全）
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
    fn evolve_rejects_length_mismatch() {
        let swarm = SwarmUpdater::new(SwarmConfig::default());
        let outcome = belief(&[1.0, 0.0]);

        // states 多于 beliefs: 3 vs 2（旧实现静默返回 2 个状态）
        let states3 = vec![BetaState::uniform_prior(); 3];
        let beliefs2 = vec![belief(&[0.5, 0.5]); 2];
        assert_eq!(
            swarm.evolve(&states3, &beliefs2, &outcome),
            Err(BayesianError::LengthMismatch {
                states_len: 3,
                beliefs_len: 2
            })
        );

        // 反向: 2 vs 3
        let states2 = vec![BetaState::uniform_prior(); 2];
        let beliefs3 = vec![belief(&[0.5, 0.5]); 3];
        assert_eq!(
            swarm.evolve(&states2, &beliefs3, &outcome),
            Err(BayesianError::LengthMismatch {
                states_len: 2,
                beliefs_len: 3
            })
        );
    }

    #[test]
    fn evolve_rejects_belief_outcome_dimension_mismatch() {
        // 第三处截断点: 内层点积 zip。3 选项 belief vs 2 选项 outcome
        let swarm = SwarmUpdater::new(SwarmConfig::default());
        let states = vec![BetaState::uniform_prior()];
        let beliefs = vec![belief(&[0.5, 0.3, 0.2])];
        let outcome = belief(&[1.0, 0.0]);
        assert_eq!(
            swarm.evolve(&states, &beliefs, &outcome),
            Err(BayesianError::Scoring(ScoringError::DimensionMismatch))
        );
    }

    #[test]
    fn evolve_equal_lengths_proceeds() {
        // happy path: 等长时正常演化且输出数量一致
        let swarm = SwarmUpdater::new(SwarmConfig::default());
        let states = vec![BetaState::uniform_prior(); 4];
        let beliefs = vec![belief(&[0.9, 0.1]); 4];
        let outcome = belief(&[1.0, 0.0]);
        let evolved = swarm.evolve(&states, &beliefs, &outcome).unwrap();
        assert_eq!(evolved.len(), 4);
        for s in &evolved {
            assert!(s.alpha() > Decimal::ZERO && s.beta() > Decimal::ZERO);
        }
    }

    #[test]
    fn evolve_empty_inputs_ok() {
        // 0 vs 0 是合法输入（无 agent），应返回空而非报错
        let swarm = SwarmUpdater::new(SwarmConfig::default());
        let evolved = swarm.evolve(&[], &[], &belief(&[1.0, 0.0])).unwrap();
        assert_eq!(evolved, [] as [BetaState; 0]);
    }
}

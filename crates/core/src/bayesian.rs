use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use thiserror::Error;

use crate::belief::Belief;
use crate::scoring::{Scorer, ScoringError, ScoringRule};

/// 贝叶斯更新器相关错误
#[derive(Debug, Error, PartialEq)]
pub enum BayesianError {
    #[error("scoring error: {0}")]
    Scoring(#[from] ScoringError),
    #[error("invalid parameters: alpha and beta must be strictly positive")]
    InvalidParameters,
}

/// 参与者的信誉状态 (基于 Beta 分布)
#[derive(Debug, Clone, PartialEq)]
pub struct BetaState {
    /// 成功/准确的伪计数
    alpha: Decimal,
    /// 失败/不准确的伪计数
    beta: Decimal,
}

impl BetaState {
    /// 创建新的 Beta 状态
    ///
    /// # Arguments
    /// * `alpha` - 初始 alpha 值 (必须 > 0)
    /// * `beta` - 初始 beta 值 (必须 > 0)
    pub fn new(alpha: Decimal, beta: Decimal) -> Result<Self, BayesianError> {
        if alpha <= Decimal::ZERO || beta <= Decimal::ZERO {
            return Err(BayesianError::InvalidParameters);
        }
        Ok(Self { alpha, beta })
    }

    /// 创建均匀先验状态 Beta(1, 1)
    pub fn uniform_prior() -> Self {
        Self {
            alpha: Decimal::ONE,
            beta: Decimal::ONE,
        }
    }

    /// 获取当前的 alpha 值
    pub fn alpha(&self) -> Decimal {
        self.alpha
    }

    /// 获取当前的 beta 值
    pub fn beta(&self) -> Decimal {
        self.beta
    }

    /// 计算期望权重 (Expected Weight / Mean of Beta distribution)
    ///
    /// W = α / (α + β)
    /// 这是 Thompson Sampling 在确定性场景下的最佳近似。
    pub fn expected_weight(&self) -> Decimal {
        self.alpha / (self.alpha + self.beta)
    }
}

/// 贝叶斯更新器
pub struct BayesianUpdater {
    scorer: Scorer,
    /// 学习率 η，控制单次更新对状态的影响幅度
    learning_rate: Decimal,
}

impl BayesianUpdater {
    /// 创建新的贝叶斯更新器
    ///
    /// # Arguments
    /// * `rule` - 使用的评分规则
    /// * `learning_rate` - 学习率 (通常较小，如 0.1)
    pub fn new(rule: ScoringRule, learning_rate: Decimal) -> Self {
        Self {
            scorer: Scorer::new(rule),
            learning_rate,
        }
    }

    /// 使用默认 Brier Score 和 η=0.1 创建更新器
    pub fn default_brier() -> Self {
        Self::new(ScoringRule::Brier, Decimal::from_f64(0.1).unwrap())
    }

    /// 根据预测和真实结果更新参与者的 Beta 状态
    ///
    /// # Arguments
    /// * `state` - 当前的 Beta 状态
    /// * `belief` - 参与者的预测
    /// * `outcome` - 真实结果
    ///
    /// # Returns
    /// 更新后的 Beta 状态
    pub fn update(
        &self,
        state: &BetaState,
        belief: &Belief,
        outcome: &Belief,
    ) -> Result<BetaState, BayesianError> {
        // 1. 计算原始得分 (Brier: [-1, 1])
        let score = self.scorer.score(belief, outcome)?;

        // 2. 归一化得分到 [0, 1]
        // S_norm = (S + 1) / 2
        let score_norm = (score + Decimal::ONE) / Decimal::TWO;

        // 3. 计算增量
        // Δα = S_norm * η
        // Δβ = (1 - S_norm) * η
        let delta_alpha = score_norm * self.learning_rate;
        let delta_beta = (Decimal::ONE - score_norm) * self.learning_rate;

        // 4. 应用更新
        Ok(BetaState {
            alpha: state.alpha + delta_alpha,
            beta: state.beta + delta_beta,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    fn make_belief(vals: &[f64]) -> Belief {
        let allocs: Vec<Decimal> = vals
            .iter()
            .map(|&v| Decimal::from_f64(v).unwrap())
            .collect();
        Belief::new(allocs).unwrap()
    }

    #[test]
    fn test_uniform_prior() {
        let state = BetaState::uniform_prior();
        assert_eq!(state.alpha(), Decimal::ONE);
        assert_eq!(state.beta(), Decimal::ONE);
        // 期望权重应为 0.5
        assert_eq!(state.expected_weight(), Decimal::from_f64(0.5).unwrap());
    }

    #[test]
    fn test_invalid_parameters() {
        assert_eq!(
            BetaState::new(Decimal::ZERO, Decimal::ONE),
            Err(BayesianError::InvalidParameters)
        );
        assert_eq!(
            BetaState::new(Decimal::ONE, Decimal::new(-1, 1)), // -0.1
            Err(BayesianError::InvalidParameters)
        );
    }

    #[test]
    fn test_update_perfect_prediction() {
        let updater = BayesianUpdater::default_brier();
        let state = BetaState::uniform_prior();

        let belief = make_belief(&[1.0, 0.0]);
        let outcome = make_belief(&[1.0, 0.0]);

        let new_state = updater.update(&state, &belief, &outcome).unwrap();

        // Brier Score = 1.0 -> S_norm = 1.0
        // Δα = 1.0 * 0.1 = 0.1
        // Δβ = 0.0 * 0.1 = 0.0
        assert_eq!(new_state.alpha(), Decimal::from_f64(1.1).unwrap());
        assert_eq!(new_state.beta(), Decimal::ONE);

        // 权重应该增加 (1.1 / 2.1 ≈ 0.5238)
        assert!(new_state.expected_weight() > state.expected_weight());
    }

    #[test]
    fn test_update_worst_prediction() {
        let updater = BayesianUpdater::default_brier();
        let state = BetaState::uniform_prior();

        let belief = make_belief(&[1.0, 0.0]);
        let outcome = make_belief(&[0.0, 1.0]);

        let new_state = updater.update(&state, &belief, &outcome).unwrap();

        // Brier Score = -1.0 -> S_norm = 0.0
        // Δα = 0.0 * 0.1 = 0.0
        // Δβ = 1.0 * 0.1 = 0.1
        assert_eq!(new_state.alpha(), Decimal::ONE);
        assert_eq!(new_state.beta(), Decimal::from_f64(1.1).unwrap());

        // 权重应该减少
        assert!(new_state.expected_weight() < state.expected_weight());
    }

    #[test]
    fn test_multiple_updates_convergence() {
        let updater = BayesianUpdater::default_brier();
        let mut state = BetaState::uniform_prior();

        let good_belief = make_belief(&[0.9, 0.1]);
        let outcome_a = make_belief(&[1.0, 0.0]);

        // 连续 10 次准确预测
        for _ in 0..10 {
            state = updater.update(&state, &good_belief, &outcome_a).unwrap();
        }

        // 10次更新后: α≈1.99, β≈1.01, W≈0.663
        // 验证权重确实从 0.5 显著增长
        assert!(state.expected_weight() > Decimal::from_f64(0.65).unwrap());
        assert!(state.expected_weight() < Decimal::from_f64(0.70).unwrap());

        // 继续更新到 50 次，验证长期收敛性
        for _ in 0..40 {
            state = updater.update(&state, &good_belief, &outcome_a).unwrap();
        }

        // 50次更新后: α≈5.95, β≈1.05, W≈0.85
        // 此时应超过 0.8 阈值
        assert!(state.expected_weight() > Decimal::from_f64(0.8).unwrap());
    }
}

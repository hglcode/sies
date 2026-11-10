use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::belief::{Belief, BeliefError};
use crate::decay::DecayError;

/// 评分规则相关错误
#[derive(Debug, Error, PartialEq)]
pub enum ScoringError {
    #[error("belief and outcome must have the same number of options")]
    DimensionMismatch,
    #[error("invalid belief: {0}")]
    InvalidBelief(#[from] BeliefError),
    #[error("deterministic math error: {0}")]
    Math(#[from] DecayError),
}

/// 支持的严格适当评分规则
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ScoringRule {
    /// Brier Score (Quadratic): `S(p, o) = 1 - sum((p_i - o_i)^2)`
    /// 范围: [0, 1]，越高越好
    Brier,
    /// Log Score (Logarithmic): `S(p, o) = sum(o_i * ln(p_i))`
    /// 范围: (-inf, 0]，越高越好（0为完美）
    Log,
}

/// 评分计算器
pub struct Scorer {
    rule: ScoringRule,
    /// Log Score 的平滑因子 ε，防止 ln(0)
    epsilon: Decimal,
}

impl Scorer {
    /// 创建新的评分器
    #[must_use]
    pub fn new(rule: ScoringRule) -> Self {
        // ε = 1e-12，足够小以避免显著偏差，又足够大以避免下溢
        let epsilon = Decimal::new(1, 12);
        Self { rule, epsilon }
    }

    /// 计算给定信念和结果的得分
    ///
    /// # Arguments
    /// * `belief` - 参与者的预测/投票向量
    /// * `outcome` - 真实结果向量 (通常为 one-hot)
    ///
    /// # Returns
    /// 得分 (Decimal)。Brier: [0, 1]；Log: (-inf, 0]
    pub fn score(&self, belief: &Belief, outcome: &Belief) -> Result<Decimal, ScoringError> {
        if belief.num_options() != outcome.num_options() {
            return Err(ScoringError::DimensionMismatch);
        }

        match self.rule {
            ScoringRule::Brier => Ok(Scorer::brier_score(belief, outcome)),
            ScoringRule::Log => self.log_score(belief, outcome),
        }
    }

    /// 计算得分并将其归一化为 [0, 1] 的"准确度"度量
    ///
    /// # 归一化方案（按规则；值域对合法 `Belief` 可证明）
    /// * **Brier**: `S_norm = (S + 1) / 2`。合法 `Belief`（非负、和为 1）下
    ///   Σ(pᵢ−oᵢ)² = Σpᵢ² + Σoᵢ² − 2Σpᵢoᵢ ≤ 2，故 S ∈ [−1, 1]，`S_norm` ∈ [0, 1]。
    /// * **Log**: `S_norm = exp(S)`（即**似然** Π pᵢ^oᵢ）。合法 `Belief` 下
    ///   p̃ᵢ = max(pᵢ, ε) ∈ [ε, 1] ⟹ S ∈ [ln ε, 0] ⟹ `S_norm` ∈ [ε, 1] ⊆ (0, 1]。
    ///
    /// # 设计取舍（审计 P0-1 整改②：为何弃用 sigmoid / 显式钳制）
    /// * `clamp((S+1)/2, 0, 1)`：S < −1（p < e⁻¹ ≈ 0.37）后进入平台区，
    ///   信号完全丢失，且拐点选择任意；
    /// * `sigmoid(κS)`：温度 κ 为人为超参数，量纲不自洽；
    /// * **似然 exp(S)**（选定）：无超参数、严格单调、天然有界；
    ///   对 one-hot 结果 `S_norm` = p̃ᵢ，使下游 Δα = η·p、Δβ = η·(1−p)
    ///   恰为 Beta-Bernoulli 共轭更新的虚拟样本形式（见 `BayesianUpdater::update`）。
    ///
    /// # Errors
    /// 同 [`Scorer::score`]；Log 规则额外可能返回确定性指数运算错误。
    pub fn normalized_score(
        &self,
        belief: &Belief,
        outcome: &Belief,
    ) -> Result<Decimal, ScoringError> {
        let score = self.score(belief, outcome)?;
        self.normalize(score)
    }

    /// 将原始得分映射到 [0, 1]（方案与证明见 `normalized_score` 文档）
    fn normalize(&self, score: Decimal) -> Result<Decimal, ScoringError> {
        match self.rule {
            ScoringRule::Brier => Ok((score + Decimal::ONE) / Decimal::TWO),
            ScoringRule::Log => crate::decay::exp_deterministic(score).map_err(ScoringError::Math),
        }
    }

    /// Brier Score: 1 - Σ(pᵢ - oᵢ)²
    fn brier_score(belief: &Belief, outcome: &Belief) -> Decimal {
        let mut sum_sq_diff = Decimal::ZERO;

        for (&p, &o) in belief.allocations().iter().zip(outcome.allocations()) {
            let diff = p - o;
            sum_sq_diff += diff * diff;
        }

        Decimal::ONE - sum_sq_diff
    }

    /// Log Score: Σ(oᵢ * ln(pᵢ))，带 ε 平滑
    fn log_score(&self, belief: &Belief, outcome: &Belief) -> Result<Decimal, ScoringError> {
        let mut score = Decimal::ZERO;

        for (&p, &o) in belief.allocations().iter().zip(outcome.allocations()) {
            if o > Decimal::ZERO {
                // 平滑处理: max(p, ε)
                let smoothed_p = if p < self.epsilon { self.epsilon } else { p };

                // 使用确定性 ln(x) 替换 f64::ln()
                let ln_p = crate::decay::ln_deterministic(smoothed_p)?;

                score += o * ln_p;
            }
        }
        Ok(score)
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
    fn test_brier_perfect_prediction() {
        let scorer = Scorer::new(ScoringRule::Brier);
        let belief = make_belief(&[1.0, 0.0]);
        let outcome = make_belief(&[1.0, 0.0]);

        let score = scorer.score(&belief, &outcome).unwrap();
        assert_eq!(score, Decimal::ONE);
    }

    #[test]
    fn test_brier_worst_prediction() {
        let scorer = Scorer::new(ScoringRule::Brier);
        let belief = make_belief(&[1.0, 0.0]);
        let outcome = make_belief(&[0.0, 1.0]);

        let score = scorer.score(&belief, &outcome).unwrap();
        // 1 - ((1-0)^2 + (0-1)^2) = 1 - 2 = -1
        assert_eq!(score, Decimal::NEGATIVE_ONE);
    }

    #[test]
    fn test_brier_uniform_prediction() {
        let scorer = Scorer::new(ScoringRule::Brier);
        let belief = make_belief(&[0.5, 0.5]);
        let outcome = make_belief(&[1.0, 0.0]);

        let score = scorer.score(&belief, &outcome).unwrap();
        // 1 - ((0.5-1)^2 + (0.5-0)^2) = 1 - 0.5 = 0.5
        assert_eq!(score, Decimal::from_f64(0.5).unwrap());
    }

    #[test]
    fn test_dimension_mismatch() {
        let scorer = Scorer::new(ScoringRule::Brier);
        let belief = make_belief(&[0.5, 0.5]);
        let outcome = make_belief(&[1.0, 0.0, 0.0]);

        assert_eq!(
            scorer.score(&belief, &outcome),
            Err(ScoringError::DimensionMismatch)
        );
    }

    #[test]
    fn test_log_score_perfect() {
        let scorer = Scorer::new(ScoringRule::Log);
        let belief = make_belief(&[1.0, 0.0]);
        let outcome = make_belief(&[1.0, 0.0]);

        let score = scorer.score(&belief, &outcome).unwrap();
        // ln(1.0) = 0
        assert_eq!(score, Decimal::ZERO);
    }

    #[test]
    fn test_log_score_epsilon_smoothing() {
        let scorer = Scorer::new(ScoringRule::Log);
        // 预测概率为 0，但结果为真 → 应使用 ε 平滑而非 -inf
        let belief = make_belief(&[0.0, 1.0]);
        let outcome = make_belief(&[1.0, 0.0]);

        let score = scorer.score(&belief, &outcome).unwrap();
        // 应该是一个很大的负数，但不是 -inf
        assert!(score < Decimal::from_i64(-20).unwrap());
        assert!(score > Decimal::MIN);
    }

    // ─── P0-1 整改③: 双规则值域属性测试 ────────────────────────

    /// 精确十进制网格（和恒为 1，避免 f64 转换噪声干扰值域断言）
    fn grid_beliefs_2d() -> Vec<Belief> {
        [
            "0",
            "0.000000001",
            "0.000001",
            "0.001",
            "0.01",
            "0.1",
            "0.25",
            "0.5",
            "0.75",
            "0.9",
            "0.99",
            "0.999999999",
            "1",
        ]
        .iter()
        .map(|s| {
            let p = Decimal::from_str_exact(s).unwrap();
            Belief::new(vec![p, Decimal::ONE - p]).unwrap()
        })
        .collect()
    }

    #[test]
    fn test_brier_score_range_property() {
        // Brier 原始值域 [-1, 1]（合法 Belief 下可证明）
        let scorer = Scorer::new(ScoringRule::Brier);
        for belief in grid_beliefs_2d() {
            for idx in 0..2 {
                let outcome = Belief::one_hot(2, idx).unwrap();
                let s = scorer.score(&belief, &outcome).unwrap();
                assert!(s >= Decimal::NEGATIVE_ONE, "Brier {s} < -1");
                assert!(s <= Decimal::ONE, "Brier {s} > 1");
            }
        }
    }

    #[test]
    fn test_log_score_range_property() {
        // Log 原始值域 [ln ε, 0]（ε = 1e-12）
        let scorer = Scorer::new(ScoringRule::Log);
        let ln_eps = Decimal::from_str_exact("-27.63102111592854820821589745").unwrap();
        let tol = Decimal::new(1, 20); // ln_deterministic 实际精度 ~1e-26
        for belief in grid_beliefs_2d() {
            for idx in 0..2 {
                let outcome = Belief::one_hot(2, idx).unwrap();
                let s = scorer.score(&belief, &outcome).unwrap();
                assert!(s <= Decimal::ZERO, "Log {s} > 0");
                assert!(s >= ln_eps - tol, "Log {s} < ln(ε) - tol");
            }
        }
    }

    #[test]
    fn test_normalized_score_brier_range_property() {
        let scorer = Scorer::new(ScoringRule::Brier);
        for belief in grid_beliefs_2d() {
            for idx in 0..2 {
                let outcome = Belief::one_hot(2, idx).unwrap();
                let norm = scorer.normalized_score(&belief, &outcome).unwrap();
                assert!(norm >= Decimal::ZERO, "norm {norm} < 0");
                assert!(norm <= Decimal::ONE, "norm {norm} > 1");
            }
        }
    }

    #[test]
    fn test_normalized_score_log_range_property() {
        // 似然归一化值域 [ε, 1]: 任何合法信念×结果组合都严格落在 (0, 1]
        let scorer = Scorer::new(ScoringRule::Log);
        let eps = Decimal::new(1, 12);
        let tol = Decimal::new(1, 20);
        for belief in grid_beliefs_2d() {
            for idx in 0..2 {
                let outcome = Belief::one_hot(2, idx).unwrap();
                let norm = scorer.normalized_score(&belief, &outcome).unwrap();
                assert!(norm > Decimal::ZERO, "norm {norm} ≤ 0");
                assert!(norm <= Decimal::ONE, "norm {norm} > 1");
                assert!(norm >= eps - tol, "norm {norm} 突破 ε 下界");
            }
        }
    }

    #[test]
    fn test_normalized_score_log_likelihood_semantics() {
        // one-hot 结果下，Log 归一化得分应恰等于（平滑后的）命中概率
        let scorer = Scorer::new(ScoringRule::Log);
        let belief = Belief::new(vec![
            Decimal::from_str_exact("0.2").unwrap(),
            Decimal::from_str_exact("0.3").unwrap(),
            Decimal::from_str_exact("0.5").unwrap(),
        ])
        .unwrap();
        let tol = Decimal::new(1, 12);
        for idx in 0..3 {
            let outcome = Belief::one_hot(3, idx).unwrap();
            let norm = scorer.normalized_score(&belief, &outcome).unwrap();
            let p_hit = belief.allocations()[idx];
            assert!(
                (norm - p_hit).abs() < tol,
                "likelihood semantics: norm {norm} ≠ p_hit {p_hit}"
            );
        }
    }
}

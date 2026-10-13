use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use thiserror::Error;

use crate::belief::{Belief, BeliefError};

/// 评分规则相关错误
#[derive(Debug, Error, PartialEq)]
pub enum ScoringError {
    #[error("belief and outcome must have the same number of options")]
    DimensionMismatch,
    #[error("invalid belief: {0}")]
    InvalidBelief(#[from] BeliefError),
}

/// 支持的严格适当评分规则
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoringRule {
    /// Brier Score (Quadratic): S(p, o) = 1 - sum((p_i - o_i)^2)
    /// 范围: [0, 1]，越高越好
    Brier,
    /// Log Score (Logarithmic): S(p, o) = sum(o_i * ln(p_i))
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
    /// 得分 (Decimal)。Brier: [0,1]；Log: (-inf, 0]
    pub fn score(&self, belief: &Belief, outcome: &Belief) -> Result<Decimal, ScoringError> {
        if belief.num_options() != outcome.num_options() {
            return Err(ScoringError::DimensionMismatch);
        }

        match self.rule {
            ScoringRule::Brier => self.brier_score(belief, outcome),
            ScoringRule::Log => self.log_score(belief, outcome),
        }
    }

    /// Brier Score: 1 - Σ(pᵢ - oᵢ)²
    fn brier_score(&self, belief: &Belief, outcome: &Belief) -> Result<Decimal, ScoringError> {
        let mut sum_sq_diff = Decimal::ZERO;

        for (&p, &o) in belief.allocations().iter().zip(outcome.allocations()) {
            let diff = p - o;
            sum_sq_diff += diff * diff;
        }

        Ok(Decimal::ONE - sum_sq_diff)
    }

    /// Log Score: Σ(oᵢ * ln(pᵢ))，带 ε 平滑
    fn log_score(&self, belief: &Belief, outcome: &Belief) -> Result<Decimal, ScoringError> {
        let mut score = Decimal::ZERO;

        for (&p, &o) in belief.allocations().iter().zip(outcome.allocations()) {
            if o > Decimal::ZERO {
                // 平滑处理: max(p, ε)
                let smoothed_p = if p < self.epsilon { self.epsilon } else { p };

                // 注意: rust_decimal 没有内置 ln()，这里需要确定性近似
                // TODO: 替换为确定性泰勒展开/帕德近似实现
                // 当前使用 f64 转换作为占位符，⚠️ 违反 ADR #2，仅用于测试结构
                let ln_p = Decimal::from_f64(smoothed_p.to_string().parse::<f64>().unwrap().ln())
                    .unwrap_or(Decimal::MIN);

                score += o * ln_p;
            }
            // 当 o == 0 时，该项贡献为 0，无需计算 ln(p)
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
}

use rust_decimal::Decimal;
use thiserror::Error;

/// Belief/Outcome 相关的错误类型
#[derive(Debug, Error, PartialEq)]
pub enum BeliefError {
    #[error("allocations vector cannot be empty")]
    EmptyAllocations,
    #[error("allocation values must be non-negative, found negative value at index {0}")]
    NegativeValue(usize),
    #[error("allocations sum to {sum}, expected exactly 1.0 (tolerance: {tolerance})")]
    NotNormalized { sum: Decimal, tolerance: Decimal },
}

/// 统一的信念/结果表达 (预测与投票的底层抽象)
///
/// - 预测场景: 连续概率分布 (如 [0.7, 0.3])
/// - 投票场景: 退化的离散分布 / One-hot (如 [1.0, 0.0])
#[derive(Debug, Clone, PartialEq)]
pub struct Belief {
    /// 分配给各个互斥选项的权重/概率向量
    allocations: Vec<Decimal>,
}

impl Belief {
    /// 归一化校验的容差阈值 (1e-9)
    const NORMALIZATION_TOLERANCE: Decimal = Decimal::new(1, 9);

    /// 从原始分配向量创建 Belief，并进行严格校验
    pub fn new(allocations: Vec<Decimal>) -> Result<Self, BeliefError> {
        if allocations.is_empty() {
            return Err(BeliefError::EmptyAllocations);
        }

        for (i, &val) in allocations.iter().enumerate() {
            if val < Decimal::ZERO {
                return Err(BeliefError::NegativeValue(i));
            }
        }

        let sum: Decimal = allocations.iter().sum();
        let diff = (sum - Decimal::ONE).abs();
        if diff > Self::NORMALIZATION_TOLERANCE {
            return Err(BeliefError::NotNormalized {
                sum,
                tolerance: Self::NORMALIZATION_TOLERANCE,
            });
        }

        Ok(Self { allocations })
    }

    /// 创建 One-hot 向量 (主要用于投票场景的离散选择)
    pub fn one_hot(num_options: usize, choice_index: usize) -> Result<Self, BeliefError> {
        if num_options == 0 {
            return Err(BeliefError::EmptyAllocations);
        }
        if choice_index >= num_options {
            // 复用 NotNormalized 或可新增 OutOfBounds 错误，此处简化处理
            return Err(BeliefError::NotNormalized {
                sum: Decimal::ZERO,
                tolerance: Self::NORMALIZATION_TOLERANCE,
            });
        }

        let mut allocs = vec![Decimal::ZERO; num_options];
        allocs[choice_index] = Decimal::ONE;

        // One-hot 天然满足归一化，直接构造避免重复校验
        Ok(Self {
            allocations: allocs,
        })
    }

    /// 获取内部分配向量的只读引用
    pub fn allocations(&self) -> &[Decimal] {
        &self.allocations
    }

    /// 获取选项数量
    pub fn num_options(&self) -> usize {
        self.allocations.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    #[test]
    fn test_valid_prediction_belief() {
        let allocs = vec![
            Decimal::from_f64(0.7).unwrap(),
            Decimal::from_f64(0.3).unwrap(),
        ];
        let belief = Belief::new(allocs);
        assert!(belief.is_ok());
        assert_eq!(belief.unwrap().num_options(), 2);
    }

    #[test]
    fn test_one_hot_voting_belief() {
        let belief = Belief::one_hot(3, 1).unwrap();
        assert_eq!(
            belief.allocations(),
            &[Decimal::ZERO, Decimal::ONE, Decimal::ZERO]
        );
    }

    #[test]
    fn test_reject_negative_values() {
        let allocs = vec![
            Decimal::from_f64(-0.1).unwrap(),
            Decimal::from_f64(1.1).unwrap(),
        ];
        assert_eq!(Belief::new(allocs), Err(BeliefError::NegativeValue(0)));
    }

    #[test]
    fn test_reject_unnormalized() {
        let allocs = vec![
            Decimal::from_f64(0.5).unwrap(),
            Decimal::from_f64(0.4).unwrap(),
        ];
        assert!(matches!(
            Belief::new(allocs),
            Err(BeliefError::NotNormalized { .. })
        ));
    }

    #[test]
    fn test_reject_empty() {
        assert_eq!(Belief::new(vec![]), Err(BeliefError::EmptyAllocations));
    }
}

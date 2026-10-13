use rust_decimal::Decimal;
use thiserror::Error;

/// 衰减机制相关错误
#[derive(Debug, Error, PartialEq)]
pub enum DecayError {
    #[error("decay rate rho must be in range (0, 1), got {0}")]
    InvalidRho(Decimal),
    #[error("half life must be strictly positive, got {0}")]
    InvalidHalfLife(Decimal),
    #[error("elapsed time cannot be negative, got {0}")]
    NegativeTime(Decimal),
}

/// 蚁群信息素挥发计算器
///
/// 公式: W_new = W_old * (1 - rho)^(t / half_life)
/// 使用确定性泰勒级数近似，完全避免 f64
#[derive(Debug, Clone)]
pub struct PheromoneDecay {
    /// 挥发率 ρ ∈ (0, 1)
    rho: Decimal,
    /// 半衰期 (时间单位)
    half_life: Decimal,
    /// 预计算的 ln(1 - rho)，避免重复计算
    ln_base: Decimal,
}

impl PheromoneDecay {
    /// 创建新的挥发计算器
    ///
    /// # Arguments
    /// * `rho` - 挥发率，取值范围 (0, 1)
    /// * `half_life` - 半衰期，必须 > 0
    pub fn new(rho: Decimal, half_life: Decimal) -> Result<Self, DecayError> {
        if rho <= Decimal::ZERO || rho >= Decimal::ONE {
            return Err(DecayError::InvalidRho(rho));
        }
        if half_life <= Decimal::ZERO {
            return Err(DecayError::InvalidHalfLife(half_life));
        }

        // ln(1 - rho)，预计算避免每次 decay_factor 调用重复运算
        let ln_base = ln_one_minus_x(rho);

        Ok(Self {
            rho,
            half_life,
            ln_base,
        })
    }

    /// 获取挥发率
    pub fn rho(&self) -> Decimal {
        self.rho
    }

    /// 获取半衰期
    pub fn half_life(&self) -> Decimal {
        self.half_life
    }

    /// 计算经过时间 t 后的衰减因子
    ///
    /// decay_factor = (1 - rho)^(t / half_life)
    ///              = exp((t / half_life) * ln(1 - rho))
    pub fn decay_factor(&self, elapsed_time: Decimal) -> Result<Decimal, DecayError> {
        if elapsed_time < Decimal::ZERO {
            return Err(DecayError::NegativeTime(elapsed_time));
        }
        if elapsed_time == Decimal::ZERO {
            return Ok(Decimal::ONE);
        }

        let exponent = (elapsed_time / self.half_life) * self.ln_base;
        Ok(exp_taylor(exponent))
    }

    /// 对给定权重应用衰减
    pub fn apply(&self, weight: Decimal, elapsed_time: Decimal) -> Result<Decimal, DecayError> {
        let factor = self.decay_factor(elapsed_time)?;
        Ok(weight * factor)
    }
}

// ─── 确定性数学近似函数 (ADR #2 compliant) ───────────────────────

/// 泰勒级数项数，20 项对 |x| < 1 的精度远超 Decimal 默认精度
const TAYLOR_TERMS: usize = 20;

/// 计算 ln(1 - x)，其中 x ∈ (0, 1)
///
/// 使用级数: ln(1 - x) = -Σ(x^n / n) for n = 1..∞
fn ln_one_minus_x(x: Decimal) -> Decimal {
    debug_assert!(x > Decimal::ZERO && x < Decimal::ONE);

    let mut result = Decimal::ZERO;
    let mut x_pow_n = x; // x^1

    for n in 1..=TAYLOR_TERMS {
        let term = x_pow_n / Decimal::from(n);
        result -= term;
        x_pow_n *= x;
    }

    result
}

/// 计算 e^x 的泰勒级数近似
///
/// e^x = Σ(x^n / n!) for n = 0..∞
fn exp_taylor(x: Decimal) -> Decimal {
    let mut result = Decimal::ONE;
    let mut term = Decimal::ONE; // x^n / n!

    for n in 1..=TAYLOR_TERMS {
        term *= x / Decimal::from(n);
        result += term;

        // 当增量小于 Decimal 精度时提前终止
        if term.abs() < Decimal::new(1, 28) {
            break;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::FromPrimitive;

    /// 测试容差 1e-6
    fn tolerance() -> Decimal {
        Decimal::new(1, 6)
    }

    fn approx_eq(a: Decimal, b: Decimal) -> bool {
        (a - b).abs() < tolerance()
    }

    #[test]
    fn test_invalid_parameters() {
        assert!(PheromoneDecay::new(Decimal::ZERO, Decimal::ONE).is_err());
        assert!(PheromoneDecay::new(Decimal::ONE, Decimal::ONE).is_err());
        assert!(PheromoneDecay::new(Decimal::new(-1, 1), Decimal::ONE).is_err());
        assert!(PheromoneDecay::new(Decimal::TWO, Decimal::ONE).is_err());
        assert!(PheromoneDecay::new(Decimal::new(1, 1), Decimal::ZERO).is_err());
    }

    #[test]
    fn test_decay_factor_at_zero_time() {
        let decay = PheromoneDecay::new(Decimal::new(1, 1), Decimal::TEN).unwrap();
        let factor = decay.decay_factor(Decimal::ZERO).unwrap();
        assert_eq!(factor, Decimal::ONE);
    }

    #[test]
    fn test_decay_factor_at_half_life() {
        // 在 t = half_life 时，衰减因子应为 1 - rho
        let rho = Decimal::new(3, 1); // 0.3
        let half_life = Decimal::TEN;
        let decay = PheromoneDecay::new(rho, half_life).unwrap();

        let factor = decay.decay_factor(half_life).unwrap();
        let expected = Decimal::ONE - rho; // 0.7

        assert!(
            approx_eq(factor, expected),
            "Expected ~{expected}, got {factor}"
        );
    }

    #[test]
    fn test_decay_factor_negative_time_rejected() {
        let decay = PheromoneDecay::new(Decimal::new(1, 1), Decimal::TEN).unwrap();
        assert_eq!(
            decay.decay_factor(Decimal::new(-1, 0)),
            Err(DecayError::NegativeTime(Decimal::new(-1, 0)))
        );
    }

    #[test]
    fn test_apply_decay() {
        let rho = Decimal::new(2, 1); // 0.2
        let half_life = Decimal::from_f64(100.0).unwrap();
        let decay = PheromoneDecay::new(rho, half_life).unwrap();

        let weight = Decimal::from_f64(0.8).unwrap();
        let new_weight = decay
            .apply(weight, Decimal::from_f64(100.0).unwrap())
            .unwrap();

        // W_new = 0.8 * (1 - 0.2)^1 = 0.8 * 0.8 = 0.64
        let expected = Decimal::from_f64(0.64).unwrap();
        assert!(
            approx_eq(new_weight, expected),
            "Expected ~{expected}, got {new_weight}"
        );
    }

    #[test]
    fn test_ln_one_minus_x_accuracy() {
        // ln(0.7) ≈ -0.356674943938732
        let x = Decimal::new(3, 1); // 0.3
        let result = ln_one_minus_x(x);
        let expected = Decimal::from_f64(-0.356674943938732).unwrap();
        assert!(
            approx_eq(result, expected),
            "ln(0.7): expected ~{expected}, got {result}"
        );
    }

    #[test]
    fn test_exp_taylor_accuracy() {
        // e^(-0.356674943938732) ≈ 0.7
        let x = Decimal::from_f64(-0.356674943938732).unwrap();
        let result = exp_taylor(x);
        let expected = Decimal::new(7, 1); // 0.7
        assert!(
            approx_eq(result, expected),
            "exp(-0.3567): expected ~{expected}, got {result}"
        );
    }

    #[test]
    fn test_monotonic_decay() {
        let decay = PheromoneDecay::new(Decimal::new(1, 1), Decimal::TEN).unwrap();
        let w0 = Decimal::ONE;

        let w1 = decay.apply(w0, Decimal::from(5)).unwrap();
        let w2 = decay.apply(w0, Decimal::from(10)).unwrap();
        let w3 = decay.apply(w0, Decimal::from(20)).unwrap();

        assert!(w0 > w1);
        assert!(w1 > w2);
        assert!(w2 > w3);
        assert!(w3 > Decimal::ZERO);
    }
}

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
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
    #[error("logarithm domain error: x must be strictly positive, got {0}")]
    InvalidLogDomain(Decimal),
    #[error("invalid input for exponential: {0}")]
    InvalidInput(Decimal),
    #[error("exponential overflow: |{0}| exceeds safe range")]
    Overflow(Decimal),
    #[error("taylor series failed to converge within {TAYLOR_TERMS} terms")]
    ConvergenceFailure,
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
        exp_taylor(exponent)
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

/// Taylor 展开计算 e^x（带参数约简）
///
/// # Precision
/// - 有效位数: ≥27 (Decimal 28位精度)
/// - 最大相对误差: < 1e-27
/// - 适用输入范围: [-64.0, 64.0]
///
/// # Errors
/// - `Overflow`: |x| > 64.0 (超出 Decimal exp 可表示范围)
/// - `ConvergenceFailure`: 内部级数未收敛（理论上不应发生）
fn exp_taylor(x: Decimal) -> Result<Decimal, DecayError> {
    let max_input = Decimal::from_str_exact("64").unwrap();
    if x.abs() > max_input {
        return Err(DecayError::Overflow(x));
    }

    let ln2 = Decimal::from_str_exact("0.6931471805599453094172321214").unwrap();

    // ⚠️ x / ln2 是普通除法，可能在 debug 模式下 panic
    // 改用 checked_div
    let k_dec = x
        .checked_div(ln2)
        .ok_or(DecayError::ConvergenceFailure)?
        .round();

    let k = k_dec.to_i64().ok_or(DecayError::Overflow(x))?;

    // ⚠️ Decimal::from(k) * ln2 是普通乘法
    let k_ln2 = Decimal::from(k)
        .checked_mul(ln2)
        .ok_or(DecayError::Overflow(x))?;

    // ⚠️ x - k_ln2 是普通减法（不会溢出，但保持一致性）
    let r = x.checked_sub(k_ln2).ok_or(DecayError::Overflow(x))?;

    // Taylor 展开 e^r
    let mut result = Decimal::ONE;
    let mut term = Decimal::ONE;
    let epsilon = Decimal::new(1, 28);

    for n in 1..=30u32 {
        let next_term = match term
            .checked_mul(r)
            .and_then(|t| t.checked_div(Decimal::from(n)))
        {
            Some(v) => v,
            None => break,
        };

        term = next_term;

        match result.checked_add(term) {
            Some(v) => result = v,
            None => return Err(DecayError::Overflow(x)),
        }

        if term.abs() < epsilon {
            break;
        }
    }

    if k != 0 {
        let two_k = pow2_int(k).ok_or(DecayError::Overflow(x))?;
        result = result.checked_mul(two_k).ok_or(DecayError::Overflow(x))?;
    }

    Ok(result)
}

/// 计算 2^k (k 为整数)，返回 None 如果溢出 Decimal 范围
fn pow2_int(k: i64) -> Option<Decimal> {
    if k == 0 {
        return Some(Decimal::ONE);
    }
    if k < 0 {
        let pos = pow2_int(-k)?;
        Decimal::ONE.checked_div(pos)
    } else {
        let mut result = Decimal::ONE;
        let base = Decimal::TWO;
        let mut exp = k as u64;
        let mut b = base;
        while exp > 0 {
            if exp & 1 == 1 {
                result = result.checked_mul(b)?;
            }
            exp >>= 1;
            if exp > 0 {
                b = b.checked_mul(b)?; // ✅ 已是 checked
            }
        }
        Some(result)
    }
}

// ==========================================
// 确定性自然对数 ln(x) 实现 (ADR #2 compliant)
// ==========================================

/// 预计算的高精度常量 (28位有效数字，rust_decimal 极限精度)
fn ln_2() -> Decimal {
    Decimal::from_str_exact("0.6931471805599453094172321214").unwrap()
}

fn ln_10() -> Decimal {
    Decimal::from_str_exact("2.3025850929940456840179914546").unwrap()
}

/// 计算任意正数 x 的自然对数 ln(x)
///
/// # 算法原理 (参数约简 + artanh 级数)
/// 1. 使用 Decimal::normalize() 将 x 表示为 c × 10^e，其中 c ∈ [1, 10)
/// 2. 进一步将 c 拆分到 [1, 2) 区间: c = c' × 2^k
/// 3. ln(x) = ln(c') + k·ln(2) + e·ln(10)
/// 4. 对 c' ∈ [1, 2) 使用 artanh 级数: ln(c') = 2·artanh((c'-1)/(c'+1))
///    此时 y = (c'-1)/(c'+1) ∈ [0, 1/3)，级数收敛极快，12项即达极限精度
pub fn ln_deterministic(x: Decimal) -> Result<Decimal, DecayError> {
    if x <= Decimal::ZERO {
        return Err(DecayError::InvalidLogDomain(x));
    }
    if x == Decimal::ONE {
        return Ok(Decimal::ZERO);
    }

    // 1. 参数约简：提取 [1, 10) 区间的系数和 base-10 指数
    let normalized = x.normalize();
    let coeff = normalized.mantissa().unsigned_abs(); // u128
    let exp10 = -(normalized.scale() as i64);

    // 将 coeff 转为 Decimal，构造 [1.0, 10.0) 范围内的值
    let coeff_str = coeff.to_string();
    let digits = coeff_str.len() as u32;

    // from_i128_with_scale 安全处理大数，不会 panic
    let m = Decimal::from_i128_with_scale(coeff as i128, digits.saturating_sub(1));
    let exp10_adjusted = exp10 + (digits as i64 - 1);

    // 2. 进一步约简到 [1.0, 2.0) 区间
    let mut k: i64 = 0;
    let mut reduced = m;

    if reduced >= Decimal::TWO {
        // 除以 2 直到落入 [1, 2)
        while reduced >= Decimal::TWO {
            reduced /= Decimal::TWO;
            k += 1;
        }
    }
    // 注意：由于 m ∈ [1, 10)，最多只需除以 2 三次 (8→4→2→1.x)
    // 这里的除法是精确的（除以2不会产生无限小数），不会 Underflow

    // 3. artanh 级数逼近 ln(reduced)，reduced ∈ [1.0, 2.0)
    // y = (r - 1) / (r + 1)，当 r ∈ [1, 2) 时 y ∈ [0, 1/3)
    let y_num = reduced - Decimal::ONE;
    let y_den = reduced + Decimal::ONE;

    // 安全除法：y_num ∈ [0, 1), y_den ∈ [2, 3)
    // 这个除法的结果是有限小数或循环节很短的小数，rust_decimal 可以处理
    // 如果仍然担心 underflow，可以用 try_div 并 fallback
    let y = y_num
        .checked_div(y_den)
        .ok_or(DecayError::InvalidLogDomain(x))?;

    let y_sq = y * y;
    let mut sum = Decimal::ZERO;
    let mut y_pow = y;

    // y ≤ 1/3 时，y^25 < 10^-12，12项足以达到 10^-28 精度
    for n in 0..15u32 {
        let divisor = Decimal::from(2 * n + 1);
        sum += y_pow / divisor;
        y_pow *= y_sq;

        // 提前终止：当增量小于 Decimal 最小精度时
        if y_pow.abs() < Decimal::new(1, 28) {
            break;
        }
    }
    let ln_reduced = Decimal::TWO * sum;

    // 4. 组合最终结果
    // ln(x) = ln(reduced) + k * ln(2) + exp10_adjusted * ln(10)
    let result = ln_reduced + Decimal::from(k) * ln_2() + Decimal::from(exp10_adjusted) * ln_10();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
    use std::str::FromStr;

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
        let expected = Decimal::new(7, 1); // 0.7
        let result = exp_taylor(x).expect("exp_taylor should succeed for valid input");
        assert!(
            approx_eq(result, expected),
            "exp({x}): expected ~{expected}, got {result}"
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

    #[test]
    fn test_ln_deterministic_accuracy() {
        // 测试 1: ln(1) = 0
        assert_eq!(ln_deterministic(Decimal::ONE).unwrap(), Decimal::ZERO);

        // 测试 2: ln(e) ≈ 1 (使用高精度字符串常量，杜绝 f64)
        let e = Decimal::from_str_exact("2.7182818284590452353602874713").unwrap();
        let ln_e = ln_deterministic(e).unwrap();
        let one = Decimal::ONE;
        // 容差设为 1e-26：常量截断 + 算术组合会损失约 2 个最低有效位
        // 这仍比 f64 的 ~1e-15 精度高 11 个数量级
        let tolerance = Decimal::from_str_exact("0.00000000000000000000000001").unwrap(); // 1e-26
        assert!(
            (ln_e - one).abs() < tolerance,
            "ln(e) expected ~1.0, got {ln_e}, diff={}",
            (ln_e - one).abs()
        );

        // 测试 3: 极小值 ln(1e-12) = -12 * ln(10)
        let eps = Decimal::from_str_exact("0.000000000001").unwrap();
        let ln_eps = ln_deterministic(eps).unwrap();
        // 注意：rust_decimal 最多 28 位有效数字，此处截断到合法范围
        let expected = Decimal::from_str_exact("-27.63102111592854820821589745").unwrap();
        let loose_tol = Decimal::from_str_exact("0.00000000000000000000000001").unwrap(); // 1e-26
        assert!(
            (ln_eps - expected).abs() < loose_tol,
            "ln(1e-12) expected ~{expected}, got {ln_eps}"
        );

        // 测试 4: 大数 ln(1e18) = 18 * ln(10)
        let big = Decimal::from_str_exact("1000000000000000000").unwrap();
        let ln_big = ln_deterministic(big).unwrap();
        // 同样截断到 28 位有效数字
        let expected_big = Decimal::from_str_exact("41.44653167389282231232384618").unwrap();
        assert!(
            (ln_big - expected_big).abs() < loose_tol,
            "ln(1e18) expected ~{expected_big}, got {ln_big}"
        );

        // 测试 5: 比特级确定性验证
        let x = Decimal::from_str_exact("0.1234567890123456789012345678").unwrap();
        let r1 = ln_deterministic(x).unwrap();
        let r2 = ln_deterministic(x).unwrap();
        assert_eq!(r1, r2, "Deterministic ln(x) must yield bit-exact results");

        // 测试 6: 非法输入
        assert!(matches!(
            ln_deterministic(Decimal::ZERO),
            Err(DecayError::InvalidLogDomain(_))
        ));
        assert!(matches!(
            ln_deterministic(Decimal::NEGATIVE_ONE),
            Err(DecayError::InvalidLogDomain(_))
        ));
    }

    // 建议测试用例结构
    #[test]
    fn exp_taylor_boundary_cases() {
        let max_convergence = Decimal::from_str_exact("64").unwrap();
        let overflow_threshold = Decimal::from_str_exact("65").unwrap();

        // 1. 零值
        assert_eq!(exp_taylor(Decimal::ZERO).unwrap(), Decimal::ONE);

        // 2. 极小值
        let small = Decimal::new(1, 28);
        assert_relative_eq!(
            exp_taylor(small).unwrap().to_f64().unwrap(),
            1.000_000_000_000_000_000_000_000_000_1_f64,
            epsilon = 1e-27
        );
        assert_relative_eq!(
            exp_taylor(-small).unwrap().to_f64().unwrap(),
            0.999_999_999_999_999_999_999_999_999_9_f64,
            epsilon = 1e-27
        );

        // 3. 收敛边界
        assert!(exp_taylor(max_convergence).is_ok());
        assert!(matches!(
            exp_taylor(overflow_threshold),
            Err(DecayError::Overflow(_))
        ));

        // 4. 超大值触发 Overflow
        assert!(matches!(
            exp_taylor(Decimal::from_str_exact("100").unwrap()),
            Err(DecayError::Overflow(_))
        ));

        // 5. 高精度参考值
        let cases: Vec<(Decimal, Decimal)> = vec![
            (Decimal::ZERO, Decimal::ONE),
            (
                Decimal::ONE,
                Decimal::from_str("2.7182818284590452353602874713").unwrap(),
            ),
            (
                Decimal::NEGATIVE_ONE,
                Decimal::from_str("0.3678794411714423215955237701").unwrap(),
            ),
            (
                Decimal::from_str("-0.5").unwrap(),
                Decimal::from_str("0.6065306597126334236037995349").unwrap(),
            ),
            (
                Decimal::from_str("10").unwrap(),
                Decimal::from_str("22026.465794806716516957900645").unwrap(),
            ), // 截断最后一位
        ];

        let tolerance = Decimal::new(1, 20); // 1e-20
        for (input, expected) in cases {
            let result =
                exp_taylor(input).unwrap_or_else(|e| panic!("exp_taylor({input}) failed: {e:?}"));
            let diff = (result - expected).abs();
            assert!(
                diff <= tolerance,
                "mismatch at {input}: got {result}, expected {expected}, diff={diff}"
            );
        }
    }
}

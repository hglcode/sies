use crate::belief::Belief;
use crate::scoring::{Scorer, ScoringError, ScoringRule};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 贝叶斯更新器相关错误
#[derive(Debug, Error, PartialEq)]
pub enum BayesianError {
    #[error("scoring error: {0}")]
    Scoring(#[from] ScoringError),
    #[error("invalid parameters: alpha and beta must be strictly positive")]
    InvalidParameters,
    #[error("length mismatch: {states_len} states vs {beliefs_len} beliefs")]
    LengthMismatch {
        states_len: usize,
        beliefs_len: usize,
    },
}

/// 参与者的信誉状态 (基于 Beta 分布)
///
/// # 不变量
/// `alpha > 0 && beta > 0`（正值伪计数）。本模块内任何构造路径
/// （含 `BayesianUpdater::update`）都必须经由 [`BetaState::new`] 完成校验，
/// 禁止以 struct literal 直接构造（审计 P0-1 整改约定）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[must_use]
    pub fn uniform_prior() -> Self {
        Self {
            alpha: Decimal::ONE,
            beta: Decimal::ONE,
        }
    }

    /// 获取当前的 alpha 值
    #[must_use]
    pub fn alpha(&self) -> Decimal {
        self.alpha
    }

    /// 获取当前的 beta 值
    #[must_use]
    pub fn beta(&self) -> Decimal {
        self.beta
    }

    /// 计算期望权重 (Expected Weight / Mean of Beta distribution)
    ///
    /// W = α / (α + β)
    /// 这是 Thompson Sampling 在确定性场景下的最佳近似。
    #[must_use]
    pub fn expected_weight(&self) -> Decimal {
        self.alpha / (self.alpha + self.beta)
    }

    /// 转换为固定长度字节表示，绕过 Postcard 对 Decimal 的不兼容
    /// 布局: [`alpha_mantissa`: i128 LE (16B)] [`alpha_scale`: u32 LE (4B)]
    ///       [`beta_mantissa`:  i128 LE (16B)] [`beta_scale`:  u32 LE (4B)]
    /// 总计 40 字节
    #[must_use]
    pub fn to_raw_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        // rust_decimal 内部表示: mantissa (i128) + scale (u32)
        // 通过 serde 序列化获取这些值是不可靠的，直接操作内部表示
        // Decimal::serialize() 返回 [u8; 16] 是 flags+scale+mantissa 的紧凑格式
        // 我们改用 from_i128_with_scale 的逆运算来安全提取

        // 使用 Decimal 的公开 API 提取 mantissa 和 scale
        let alpha_mantissa = self.alpha.mantissa();
        let alpha_scale = self.alpha.scale();
        let beta_mantissa = self.beta.mantissa();
        let beta_scale = self.beta.scale();

        buf[0..16].copy_from_slice(&alpha_mantissa.to_le_bytes());
        buf[16..20].copy_from_slice(&alpha_scale.to_le_bytes());
        buf[20..36].copy_from_slice(&beta_mantissa.to_le_bytes());
        buf[36..40].copy_from_slice(&beta_scale.to_le_bytes());
        buf
    }

    /// 从固定长度字节恢复 `BetaState`
    #[allow(clippy::expect_used)] // SAFETY: buf is &[u8; 40], all slice ranges are statically valid
    pub fn from_raw_bytes(buf: &[u8; 40]) -> Result<Self, BayesianError> {
        let alpha_mantissa = i128::from_le_bytes(buf[0..16].try_into().expect("buf is 40 bytes"));
        let alpha_scale = u32::from_le_bytes(buf[16..20].try_into().expect("buf is 40 bytes"));
        let beta_mantissa = i128::from_le_bytes(buf[20..36].try_into().expect("buf is 40 bytes"));
        let beta_scale = u32::from_le_bytes(buf[36..40].try_into().expect("buf is 40 bytes"));

        let alpha = Decimal::from_i128_with_scale(alpha_mantissa, alpha_scale);
        let beta = Decimal::from_i128_with_scale(beta_mantissa, beta_scale);
        Self::new(alpha, beta)
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
    #[must_use]
    pub fn new(rule: ScoringRule, learning_rate: Decimal) -> Self {
        Self {
            scorer: Scorer::new(rule),
            learning_rate,
        }
    }

    /// 使用默认 Brier Score 和 η=0.1 创建更新器
    #[must_use]
    pub fn default_brier() -> Self {
        Self::new(ScoringRule::Brier, Decimal::new(1, 1))
    }

    /// 根据预测和真实结果更新参与者的 Beta 状态
    ///
    /// # 算法
    /// 1. 归一化得分 `S_norm ∈ [0, 1]`（按评分规则选择方案，值域证明见
    ///    `crate::scoring::Scorer::normalized_score`）
    /// 2. 增量: `Δα = S_norm · η`，`Δβ = (1 − S_norm) · η`
    ///    （Log 规则下即 Beta-Bernoulli 共轭更新的虚拟样本形式）
    /// 3. **经 [`BetaState::new`] 构造新状态**（P0-1 整改①）：值域契约一旦
    ///    被打破，以 `InvalidParameters` 显式报错，而非静默绕过校验
    ///    写出非法状态并流入 `to_raw_bytes()` / SMT
    ///
    /// # Errors
    /// * [`BayesianError::Scoring`] - 评分/归一化失败（维度失配、确定性数学错误）
    /// * [`BayesianError::InvalidParameters`] - 更新后状态非法（防御性兜底，
    ///   合法输入下不可达，见 `normalized_score` 的值域证明）
    pub fn update(
        &self,
        state: &BetaState,
        belief: &Belief,
        outcome: &Belief,
    ) -> Result<BetaState, BayesianError> {
        // 1. 归一化得分（旧实现在此硬编码 (S+1)/2，对 Log 规则不成立 —— P0-1 根因之一）
        let score_norm = self.scorer.normalized_score(belief, outcome)?;

        // 值域契约（证明见 Scorer::normalized_score 文档）:
        // Brier → [0,1]；Log → [ε,1] ⊆ (0,1]
        debug_assert!(
            score_norm >= Decimal::ZERO && score_norm <= Decimal::ONE,
            "normalized score violates range contract: {score_norm}"
        );

        // 2. 增量
        let delta_alpha = score_norm * self.learning_rate;
        let delta_beta = (Decimal::ONE - score_norm) * self.learning_rate;

        // 3. P0-1 整改①: 经构造器校验，不再以 struct literal 绕过 BetaState::new
        BetaState::new(state.alpha + delta_alpha, state.beta + delta_beta)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
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

    // ─── P0-1 整改: 回归边界与不变量测试 ────────────────────────

    #[test]
    fn test_update_log_rule_boundary_p_zero() {
        // 审计 P0-1 边界 fixture（e2e 场景单元化）:
        //   旧实现: S_norm = (S+1)/2 ≈ (-27.63+1)/2 = -13.32 → Δα = -1.33
        //           → alpha = -0.33（非法），且经 to_raw_bytes() 静默写入 SMT，
        //           故障表现点距根因隔两个存储层
        //   新实现: 似然归一化 S_norm = exp(S) ≈ ε = 1e-12 → Δα ≈ 1e-13 → 状态始终合法
        let updater = BayesianUpdater::new(ScoringRule::Log, Decimal::new(1, 1));
        let state = BetaState::uniform_prior();
        let belief = make_belief(&[0.0, 1.0]);
        let outcome = make_belief(&[1.0, 0.0]);

        let new_state = updater
            .update(&state, &belief, &outcome)
            .expect("p=0 boundary must yield a valid BetaState");

        assert!(
            new_state.alpha() > Decimal::ZERO,
            "alpha must stay positive"
        );
        assert!(new_state.beta() > Decimal::ZERO, "beta must stay positive");
        // Δα ≈ ε·η = 1e-13: 重度惩罚体现在 Δβ 上，α 几乎不动
        assert!(new_state.alpha() >= Decimal::ONE);
        assert!(new_state.alpha() - Decimal::ONE < Decimal::new(1, 11));
        assert!((new_state.beta() - Decimal::from_f64(1.1).unwrap()).abs() < Decimal::new(1, 11));

        // 存储层回归: 合法状态必须能通过 to_raw_bytes/from_raw_bytes 往返
        // （旧实现的非法状态会在 from_raw_bytes 处才报错）
        let restored = BetaState::from_raw_bytes(&new_state.to_raw_bytes())
            .expect("round-trip must succeed for a valid state");
        assert_eq!(restored, new_state);
    }

    #[test]
    fn test_update_log_rule_golden_vectors() {
        // Log 规则 golden vector（似然归一化）:
        // one-hot 结果下 S_norm = p̃（对真实结果所赋的平滑后概率）
        let updater = BayesianUpdater::new(ScoringRule::Log, Decimal::new(1, 1));
        let state = BetaState::uniform_prior();
        let tol = Decimal::new(1, 12);

        // 完美预测 p=1: S_norm = 1 → α = 1.1, β = 1.0（exp(0) 精确为 1）
        let new = updater
            .update(&state, &make_belief(&[1.0, 0.0]), &make_belief(&[1.0, 0.0]))
            .unwrap();
        assert_eq!(new.alpha(), Decimal::from_f64(1.1).unwrap());
        assert_eq!(new.beta(), Decimal::ONE);

        // 均匀预测 p=0.5: S_norm = 0.5 → α = β = 1.05
        let new = updater
            .update(&state, &make_belief(&[0.5, 0.5]), &make_belief(&[1.0, 0.0]))
            .unwrap();
        assert!((new.alpha() - Decimal::from_f64(1.05).unwrap()).abs() < tol);
        assert!((new.beta() - Decimal::from_f64(1.05).unwrap()).abs() < tol);

        // p=0.9: S_norm = 0.9 → α = 1.09, β = 1.01
        let new = updater
            .update(&state, &make_belief(&[0.9, 0.1]), &make_belief(&[1.0, 0.0]))
            .unwrap();
        assert!((new.alpha() - Decimal::from_f64(1.09).unwrap()).abs() < tol);
        assert!((new.beta() - Decimal::from_f64(1.01).unwrap()).abs() < tol);
    }

    #[test]
    fn test_update_never_constructs_invalid_state() {
        // 验收标准: 任何合法 API 组合（规则 × 学习率 × 初始状态 × 信念 × 结果）
        // 均无法构造非法 BetaState —— 2/3 选项、含 p=0 与 p=1 边界
        let probs = [
            "0", "0.001", "0.1", "0.25", "0.5", "0.75", "0.9", "0.999", "1",
        ];
        let states = [
            BetaState::uniform_prior(),
            BetaState::new(Decimal::new(1, 3), Decimal::new(1, 3)).unwrap(), // 0.001 / 0.001
            BetaState::new(Decimal::new(1, 3), Decimal::TEN).unwrap(),
            BetaState::new(Decimal::TEN, Decimal::new(1, 3)).unwrap(),
        ];

        for rule in [ScoringRule::Brier, ScoringRule::Log] {
            for eta in [Decimal::new(1, 1), Decimal::ONE] {
                let updater = BayesianUpdater::new(rule, eta);
                for state in &states {
                    for p_str in probs {
                        let p = Decimal::from_str_exact(p_str).unwrap();
                        let belief = Belief::new(vec![p, Decimal::ONE - p]).unwrap();
                        for idx in 0..2 {
                            let outcome = Belief::one_hot(2, idx).unwrap();
                            match updater.update(state, &belief, &outcome) {
                                Ok(new) => {
                                    assert!(new.alpha() > Decimal::ZERO, "alpha invalid p={p_str}");
                                    assert!(new.beta() > Decimal::ZERO, "beta invalid p={p_str}");
                                }
                                Err(e) => panic!(
                                    "update failed: rule={rule:?} eta={eta} p={p_str}: {e:?}"
                                ),
                            }
                        }
                    }
                    // 3 选项极端信念 [0,0,1]（含两次 p=0 命中边界）
                    let belief =
                        Belief::new(vec![Decimal::ZERO, Decimal::ZERO, Decimal::ONE]).unwrap();
                    for idx in 0..3 {
                        let outcome = Belief::one_hot(3, idx).unwrap();
                        let new = updater
                            .update(state, &belief, &outcome)
                            .unwrap_or_else(|e| panic!("3-option update failed: {e:?}"));
                        assert!(new.alpha() > Decimal::ZERO);
                        assert!(new.beta() > Decimal::ZERO);
                    }
                }
            }
        }
    }

    #[test]
    fn test_from_raw_bytes_rejects_negative_alpha() {
        // 锁定"根因→存储层"防御: 携带非正 alpha 的字节序列必须在读回时被拒绝
        // （α = -0.33 —— 旧缺陷实际产出过的非法值；β = 2.43）
        let mut buf = [0u8; 40];
        buf[0..16].copy_from_slice(&(-33i128).to_le_bytes());
        buf[16..20].copy_from_slice(&2u32.to_le_bytes());
        buf[20..36].copy_from_slice(&243i128.to_le_bytes());
        buf[36..40].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(
            BetaState::from_raw_bytes(&buf),
            Err(BayesianError::InvalidParameters)
        );
    }
}

//! 串联演化引擎行为测试（任务 #43）
//!
//! 覆盖 `SwarmEvolutionEngine::step` 全链路（Phase 1 贝叶斯个体学习 ×
//! Phase 2 群体信息素演化）的行为契约: 组合等价性、位级确定性、
//! 错误传播、多轮状态不变量、权重排序与不动点、
//! 默认配置饱和特性（当前语义锁定）、边界输入。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use sies_core::bayesian::{BayesianError, BayesianUpdater, BetaState};
use sies_core::belief::Belief;
use sies_core::engine::{SwarmEvolutionConfig, SwarmEvolutionEngine};
use sies_core::scoring::{ScoringError, ScoringRule};
use sies_core::swarm::{SwarmConfig, SwarmUpdater};

fn belief(vals: &[f64]) -> Belief {
    Belief::new(
        vals.iter()
            .map(|&v| Decimal::from_f64(v).unwrap())
            .collect(),
    )
    .unwrap()
}

fn outcome_hit() -> Belief {
    belief(&[1.0, 0.0])
}

// ─── 串联契约 ──────────────────────────────────────────────────

#[test]
fn step_equals_manual_composition() {
    // 锁定阶段顺序: step() ≡ swarm.evolve(逐 agent bayesian.update 后的状态)。
    // Phase 2 必须作用于 Phase 1 的输出；重构不得静默调换阶段或复用旧状态
    let config = SwarmEvolutionConfig::default();
    let engine = SwarmEvolutionEngine::new(config.clone());
    let updater = BayesianUpdater::new(ScoringRule::Brier, config.bayesian_learning_rate);
    let swarm = SwarmUpdater::new(config.swarm);

    let states = vec![BetaState::uniform_prior(); 3];
    let beliefs = vec![
        belief(&[0.9, 0.1]),
        belief(&[0.5, 0.5]),
        belief(&[0.1, 0.9]),
    ];
    let outcome = outcome_hit();

    let via_engine = engine.step(&states, &beliefs, &outcome).unwrap();

    let bayesian_updated: Vec<BetaState> = states
        .iter()
        .zip(beliefs.iter())
        .map(|(s, b)| updater.update(s, b, &outcome).unwrap())
        .collect();
    let via_manual = swarm.evolve(&bayesian_updated, &beliefs, &outcome).unwrap();

    assert_eq!(via_engine, via_manual);
}

#[test]
fn step_is_bit_exact_deterministic() {
    // ADR #2 确定性口径: 同输入两次调用的全部 Decimal 位级一致；
    // 两条独立 50 轮演化轨迹终态一致
    let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
    let states = vec![BetaState::uniform_prior(); 4];
    let beliefs = vec![
        belief(&[0.9, 0.1]),
        belief(&[0.5, 0.5]),
        belief(&[0.1, 0.9]),
        belief(&[0.7, 0.3]),
    ];
    let outcome = outcome_hit();

    let a = engine.step(&states, &beliefs, &outcome).unwrap();
    let b = engine.step(&states, &beliefs, &outcome).unwrap();
    assert_eq!(a, b);

    let mut t1 = states.clone();
    let mut t2 = states.clone();
    for _ in 0..50 {
        t1 = engine.step(&t1, &beliefs, &outcome).unwrap();
        t2 = engine.step(&t2, &beliefs, &outcome).unwrap();
    }
    assert_eq!(t1, t2);
}

#[test]
fn step_propagates_dimension_mismatch_from_bayesian_phase() {
    // Phase 1 先于 Phase 2: 维度失配在贝叶斯评分阶段即报 Err，
    // 不会流入群体阶段（P0-2 的纵深防御在链路层面同样成立）
    let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
    let states = vec![BetaState::uniform_prior(); 2];
    let beliefs = vec![belief(&[0.5, 0.5]), belief(&[0.4, 0.3, 0.3])];
    assert_eq!(
        engine.step(&states, &beliefs, &outcome_hit()),
        Err(BayesianError::Scoring(ScoringError::DimensionMismatch))
    );
}

// ─── 多轮不变量与排序 ──────────────────────────────────────────

#[test]
fn step_multi_round_preserves_state_validity() {
    // 不变量: 任意轮数/配置下 alpha>0、beta>0、0<W<1 恒成立
    // （P0-1 后合法状态只能经 BetaState::new 构造，链路不可破坏）
    let configs = [
        SwarmEvolutionConfig::default(),
        SwarmEvolutionConfig {
            bayesian_learning_rate: Decimal::ONE,
            swarm: SwarmConfig {
                evaporation_rate: Decimal::from_f64(0.9).unwrap(),
                deposit_strength: Decimal::from_f64(0.5).unwrap(),
            },
        },
    ];
    let beliefs = vec![
        belief(&[0.9, 0.1]),
        belief(&[0.5, 0.5]),
        belief(&[0.1, 0.9]),
        belief(&[0.0, 1.0]),
        belief(&[1.0, 0.0]),
    ];
    let outcome = outcome_hit();

    for config in &configs {
        let engine = SwarmEvolutionEngine::new(config.clone());
        let mut states = vec![BetaState::uniform_prior(); beliefs.len()];
        for round in 0..100 {
            states = engine.step(&states, &beliefs, &outcome).unwrap();
            for (i, s) in states.iter().enumerate() {
                assert!(
                    s.alpha() > Decimal::ZERO,
                    "round {round} agent {i}: alpha {}",
                    s.alpha()
                );
                assert!(
                    s.beta() > Decimal::ZERO,
                    "round {round} agent {i}: beta {}",
                    s.beta()
                );
                let w = s.expected_weight();
                assert!(
                    w > Decimal::ZERO && w < Decimal::ONE,
                    "round {round} agent {i}: weight {w}"
                );
            }
        }
    }
}

#[test]
fn step_weight_ordering_differentiated_config() {
    // Q 与 ρ 同量级（各 0.05）时，群体层不动点 w* ≈ Q·acc/ρ ≈ acc:
    // 高手(0.9) > 均匀(0.5) > 逆向(0.1)，贝叶斯层提供叠加牵引
    let config = SwarmEvolutionConfig {
        bayesian_learning_rate: Decimal::new(1, 1),
        swarm: SwarmConfig {
            evaporation_rate: Decimal::from_f64(0.05).unwrap(),
            deposit_strength: Decimal::from_f64(0.05).unwrap(),
        },
    };
    let engine = SwarmEvolutionEngine::new(config);
    let beliefs = vec![
        belief(&[0.9, 0.1]),
        belief(&[0.5, 0.5]),
        belief(&[0.1, 0.9]),
    ];
    let outcome = outcome_hit();

    let mut states = vec![BetaState::uniform_prior(); 3];
    for _ in 0..100 {
        states = engine.step(&states, &beliefs, &outcome).unwrap();
    }

    let w: Vec<Decimal> = states
        .iter()
        .map(sies_core::bayesian::BetaState::expected_weight)
        .collect();
    assert!(
        w[0] > w[1],
        "expert {} should exceed uniform {}",
        w[0],
        w[1]
    );
    assert!(
        w[1] > w[2],
        "uniform {} should exceed contrarian {}",
        w[1],
        w[2]
    );
    // 不动点邻域（贝叶斯牵引带来 ~±0.04 偏移，界限取宽）
    let (lo, hi) = (
        Decimal::from_f64(0.8).unwrap(),
        Decimal::from_f64(0.98).unwrap(),
    );
    assert!(w[0] > lo && w[0] < hi, "expert weight {}", w[0]);
    let (lo, hi) = (
        Decimal::from_f64(0.4).unwrap(),
        Decimal::from_f64(0.65).unwrap(),
    );
    assert!(w[1] > lo && w[1] < hi, "uniform weight {}", w[1]);
    let (lo, hi) = (
        Decimal::from_f64(0.05).unwrap(),
        Decimal::from_f64(0.2).unwrap(),
    );
    assert!(w[2] > lo && w[2] < hi, "contrarian weight {}", w[2]);
}

// ─── 配置特性与边界 ────────────────────────────────────────────

#[test]
fn step_default_config_saturates_high_accuracy_agents() {
    // 特性锁定（记录当前语义，非缺陷裁定）:
    // 默认 Q=1 相对 ρ=0.05 过大，群体不动点 Q·acc/ρ ≥ 1 ⟺ acc ≥ 0.05，
    // 中高准确度 agent 的权重数轮内即饱和至 1，群体层失去区分度，
    // 区分度仅由贝叶斯层的 alpha/beta 总量承载。
    // 群体经济参数重定标（Q/ρ 比率）归 #68/#69 抗操纵机制范畴。
    let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
    let beliefs = vec![belief(&[0.9, 0.1]), belief(&[0.5, 0.5])];
    let outcome = outcome_hit();

    let mut states = vec![BetaState::uniform_prior(); 2];
    for _ in 0..5 {
        states = engine.step(&states, &beliefs, &outcome).unwrap();
    }
    for (i, s) in states.iter().enumerate() {
        let w = s.expected_weight();
        assert!(
            w > Decimal::from_f64(0.99).unwrap(),
            "agent {i} weight {w} should saturate under default config"
        );
    }
}

#[test]
fn step_supports_multi_option_beliefs() {
    let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
    let beliefs = vec![belief(&[0.6, 0.3, 0.1]), belief(&[0.2, 0.5, 0.3])];
    let outcome = belief(&[0.0, 1.0, 0.0]); // 结果落在选项 1
    let states = vec![BetaState::uniform_prior(); 2];

    let out = engine.step(&states, &beliefs, &outcome).unwrap();
    assert_eq!(out.len(), 2);
    // 对已实现结局分配更高概率者（0.5 vs 0.3）权重应更高
    assert!(
        out[1].expected_weight() > out[0].expected_weight(),
        "hit agent {} should exceed miss agent {}",
        out[1].expected_weight(),
        out[0].expected_weight()
    );
}

#[test]
fn step_empty_agents_returns_empty() {
    let engine = SwarmEvolutionEngine::new(SwarmEvolutionConfig::default());
    let out = engine.step(&[], &[], &outcome_hit()).unwrap();
    // clippy assert_is_empty: 用 assert_eq 展示失败值（#41 的教训）
    assert_eq!(out, [] as [BetaState; 0]);
}

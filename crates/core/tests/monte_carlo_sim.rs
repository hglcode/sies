//! 蒙特卡洛仿真测试
//!
//! 生成 1000 个虚拟用户（高手/菜鸟/跟风者），运行 100 轮预测，
//! 验证贝叶斯权重演化是否符合预期。
//!
//! 使用固定种子 LCG PRNG 确保完全可复现、零外部依赖。

use dwes_core::bayesian::{BayesianUpdater, BetaState};
use dwes_core::belief::Belief;
use dwes_core::decay::PheromoneDecay;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

// ─── 确定性 PRNG (线性同余) ──────────────────────────────────────

struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn next_decimal(&mut self) -> Decimal {
        let val = self.next_u64();
        Decimal::from(val % 1_000_000_000) / Decimal::from(1_000_000_000u64)
    }

    fn next_decimal_range(&mut self, low: Decimal, high: Decimal) -> Decimal {
        let t = self.next_decimal();
        low + t * (high - low)
    }

    fn coin_flip(&mut self, p: Decimal) -> bool {
        self.next_decimal() < p
    }
}

// ─── 用户类型 ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserType {
    Expert,
    Novice,
    Follower,
}

struct SimUser {
    id: usize,
    user_type: UserType,
    state: BetaState,
}

// ─── 仿真参数 ────────────────────────────────────────────────────

const NUM_USERS: usize = 1000;
const NUM_ROUNDS: usize = 100;
const SEED: u64 = 42;
const NUM_OPTIONS: usize = 2;

fn assign_user_type(id: usize) -> UserType {
    match id % 10 {
        0 | 1 => UserType::Expert,
        2..=6 => UserType::Novice,
        _ => UserType::Follower,
    }
}

fn generate_belief(rng: &mut LcgRng, user_type: UserType, true_outcome_idx: usize) -> Belief {
    let accuracy = match user_type {
        UserType::Expert => Decimal::from_f64(0.85).unwrap(),
        UserType::Novice => Decimal::from_f64(0.50).unwrap(),
        UserType::Follower => Decimal::from_f64(0.65).unwrap(),
    };

    let correct = rng.coin_flip(accuracy);
    let p_true = if correct {
        rng.next_decimal_range(
            Decimal::from_f64(0.7).unwrap(),
            Decimal::from_f64(0.95).unwrap(),
        )
    } else {
        rng.next_decimal_range(
            Decimal::from_f64(0.05).unwrap(),
            Decimal::from_f64(0.3).unwrap(),
        )
    };

    let p_other = Decimal::ONE - p_true;
    let allocs = if true_outcome_idx == 0 {
        vec![p_true, p_other]
    } else {
        vec![p_other, p_true]
    };
    Belief::new(allocs).expect("generated belief must be valid")
}

fn generate_outcome(rng: &mut LcgRng) -> (Belief, usize) {
    let idx = if rng.coin_flip(Decimal::from_f64(0.6).unwrap()) {
        0
    } else {
        1
    };
    (Belief::one_hot(NUM_OPTIONS, idx).unwrap(), idx)
}

fn mean_weight(users: &[SimUser], user_type: UserType) -> Decimal {
    let filtered: Vec<Decimal> = users
        .iter()
        .filter(|u| u.user_type == user_type)
        .map(|u| u.state.expected_weight())
        .collect();
    if filtered.is_empty() {
        return Decimal::ZERO;
    }
    filtered.iter().sum::<Decimal>() / Decimal::from(filtered.len())
}

// ─── 测试 ────────────────────────────────────────────────────────

#[test]
fn test_monte_carlo_weight_evolution() {
    let mut rng = LcgRng::new(SEED);
    let updater = BayesianUpdater::default_brier();

    let mut users: Vec<SimUser> = (0..NUM_USERS)
        .map(|id| SimUser {
            id,
            user_type: assign_user_type(id),
            state: BetaState::uniform_prior(),
        })
        .collect();

    let init_expert = mean_weight(&users, UserType::Expert);
    let init_novice = mean_weight(&users, UserType::Novice);
    let init_follower = mean_weight(&users, UserType::Follower);

    println!("=== Initial Weights ===");
    println!("  Expert:   {init_expert}");
    println!("  Novice:   {init_novice}");
    println!("  Follower: {init_follower}");

    assert_eq!(init_expert, Decimal::from_f64(0.5).unwrap());
    assert_eq!(init_novice, Decimal::from_f64(0.5).unwrap());
    assert_eq!(init_follower, Decimal::from_f64(0.5).unwrap());

    for round in 0..NUM_ROUNDS {
        let (outcome, outcome_idx) = generate_outcome(&mut rng);
        for user in users.iter_mut() {
            let belief = generate_belief(&mut rng, user.user_type, outcome_idx);
            user.state = updater.update(&user.state, &belief, &outcome).unwrap();
        }
        if (round + 1) % 25 == 0 {
            let e = mean_weight(&users, UserType::Expert);
            let n = mean_weight(&users, UserType::Novice);
            let f = mean_weight(&users, UserType::Follower);
            println!(
                "Round {:>3}: Expert={e:.6}, Novice={n:.6}, Follower={f:.6}",
                round + 1
            );
        }
    }

    let final_expert = mean_weight(&users, UserType::Expert);
    let final_novice = mean_weight(&users, UserType::Novice);
    let final_follower = mean_weight(&users, UserType::Follower);

    println!("\n=== Final Weights (after {} rounds) ===", NUM_ROUNDS);
    println!("  Expert:   {final_expert}");
    println!("  Novice:   {final_novice}");
    println!("  Follower: {final_follower}");

    // ─── 核心断言 ───────────────────────────────────────────────

    // 1. 高手权重显著上升 (> 0.7)
    assert!(
        final_expert > Decimal::from_f64(0.7).unwrap(),
        "Expert weight should rise above 0.7, got {final_expert}"
    );

    // 2. 严格排序不变量: Expert > Follower > Novice
    assert!(
        final_expert > final_follower,
        "Expert ({final_expert}) should outperform Follower ({final_follower})"
    );
    assert!(
        final_follower > final_novice,
        "Follower ({final_follower}) should outperform Novice ({final_novice})"
    );

    // 3. 高手与菜鸟的权重差距应显著 (> 0.1)
    let gap = final_expert - final_novice;
    assert!(
        gap > Decimal::from_f64(0.1).unwrap(),
        "Expert-Novice gap should exceed 0.1, got {gap}"
    );

    // 4. 所有权重仍在合法范围 (0, 1)
    for user in &users {
        let w = user.state.expected_weight();
        assert!(
            w > Decimal::ZERO && w < Decimal::ONE,
            "User {} weight {w} out of range",
            user.id
        );
    }
}

#[test]
fn test_monte_carlo_with_decay() {
    let mut rng = LcgRng::new(SEED + 1);
    let updater = BayesianUpdater::default_brier();
    let decay = PheromoneDecay::new(
        Decimal::from_f64(0.1).unwrap(),
        Decimal::from_f64(50.0).unwrap(),
    )
    .unwrap();

    let mut users: Vec<SimUser> = (0..NUM_USERS)
        .map(|id| SimUser {
            id,
            user_type: assign_user_type(id),
            state: BetaState::uniform_prior(),
        })
        .collect();

    for round in 0..NUM_ROUNDS {
        let (outcome, outcome_idx) = generate_outcome(&mut rng);
        for user in users.iter_mut() {
            let belief = generate_belief(&mut rng, user.user_type, outcome_idx);
            user.state = updater.update(&user.state, &belief, &outcome).unwrap();
        }

        if (round + 1) % 10 == 0 {
            for user in users.iter_mut() {
                let w = user.state.expected_weight();
                let decayed_w = decay.apply(w, Decimal::TEN).unwrap();
                let total = user.state.alpha() + user.state.beta();
                let new_alpha = decayed_w * total;
                let new_beta = (Decimal::ONE - decayed_w) * total;
                user.state = BetaState::new(
                    new_alpha.max(Decimal::new(1, 10)),
                    new_beta.max(Decimal::new(1, 10)),
                )
                .expect("decayed state must remain valid");
            }
        }
    }

    let final_expert = mean_weight(&users, UserType::Expert);
    let final_novice = mean_weight(&users, UserType::Novice);
    let final_follower = mean_weight(&users, UserType::Follower);

    println!("\n=== Decay Simulation Results ===");
    println!("  Expert:   {final_expert}");
    println!("  Novice:   {final_novice}");
    println!("  Follower: {final_follower}");

    // 即使有衰减，排序不变量仍应保持
    assert!(
        final_expert > final_novice,
        "With decay: Expert ({final_expert}) should still outperform Novice ({final_novice})"
    );
    assert!(
        final_follower > final_novice,
        "With decay: Follower ({final_follower}) should still outperform Novice ({final_novice})"
    );
}

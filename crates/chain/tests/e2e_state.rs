#![allow(clippy::unwrap_used, clippy::expect_used)]
use rust_decimal::{Decimal, prelude::FromPrimitive};
use sies_chain::smt::SmtStateStore;
use sies_core::{
    bayesian::{BayesianUpdater, BetaState},
    belief::Belief,
};
use sies_storage::StateStore;

/// 端到端：贝叶斯更新 → SMT 持久化 → 读回验证
#[test]
fn test_bayesian_update_persisted_in_smt() {
    let mut store = SmtStateStore::new();
    let user_id = 1001u64;

    // 初始化均匀先验并持久化
    let prior = BetaState::uniform_prior();
    store.put_state(user_id, &prior).unwrap();

    // 读回验证 roundtrip
    let loaded = store
        .get_state(user_id)
        .unwrap()
        .expect("state should exist");
    assert_eq!(loaded, prior);

    // 执行一次贝叶斯更新（完美预测）
    let updater = BayesianUpdater::default_brier();
    let belief = Belief::new(vec![Decimal::from_f64(1.0).unwrap()]).unwrap();
    let outcome = Belief::new(vec![Decimal::from_f64(1.0).unwrap()]).unwrap(); // outcome 也是 Belief
    let updbeta = updater.update(&loaded, &belief, &outcome).unwrap();

    // 持久化更新后的状态
    store.put_state(user_id, &updbeta).unwrap();

    // 再次读回验证一致性
    let reloaded = store
        .get_state(user_id)
        .unwrap()
        .expect("updated state should exist");
    assert_eq!(reloaded, updbeta);
    assert!(reloaded.expected_weight() > prior.expected_weight());
}

/// 多用户状态隔离
#[test]
fn test_multi_user_state_isolation() {
    let mut store = SmtStateStore::new();

    let state_a = BetaState::uniform_prior();
    let state_b = BetaState::new(
        Decimal::from_f64(5.0).unwrap(),
        Decimal::from_f64(2.0).unwrap(),
    )
    .unwrap();

    store.put_state(1, &state_a).unwrap();
    store.put_state(2, &state_b).unwrap();

    assert_eq!(store.get_state(1).unwrap(), Some(state_a));
    assert_eq!(store.get_state(2).unwrap(), Some(state_b));
    assert_eq!(store.get_state(3).unwrap(), None);
}

/// 覆盖写入后 root hash 变化
#[test]
fn test_state_overwrite_updates_root() {
    let mut store = SmtStateStore::new();
    let user_id = 42u64;

    let v1 = BetaState::uniform_prior();
    store.put_state(user_id, &v1).unwrap();
    let root_v1 = store.root_hash();

    let v2 = BetaState::new(
        Decimal::from_f64(10.0).unwrap(),
        Decimal::from_f64(1.0).unwrap(),
    )
    .unwrap();
    store.put_state(user_id, &v2).unwrap();
    let root_v2 = store.root_hash();

    assert_ne!(root_v1, root_v2);
    assert_eq!(store.get_state(user_id).unwrap(), Some(v2));
}

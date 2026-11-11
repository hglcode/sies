//! 契约层验收测试（任务 #30）：
//! - 定点转换往返无损（BeliefVector / `LedgerEntry`）
//! - 契约工件零浮点（proto + `OpenAPI` 文本级扫描）
//! - 签名字段长度契约、统一错误模型映射、版本化拒绝策略

#![allow(clippy::unwrap_used, clippy::expect_used)]

use rust_decimal::Decimal;
use sies_core::belief::Belief;
use sies_rpc::{
    BELIEF_VECTOR_VERSION, BeliefVector, ConversionError, ED25519_PUBLIC_KEY_LEN,
    ED25519_SIGNATURE_LEN, EntryAuth, ErrorCode, FIXED_POINT_SCALE, belief_to_vector,
    decimal_from_string, ledger_entry_from_proto, ledger_entry_to_proto, vector_to_belief,
};
use sies_storage::LedgerEntry;

fn belief(allocations: Vec<Decimal>) -> Belief {
    Belief::new(allocations).unwrap()
}

fn sample_entry(alpha: Decimal) -> LedgerEntry {
    LedgerEntry::new(
        42,
        7,
        9,
        belief(vec![Decimal::new(7, 1), Decimal::new(3, 1)]),
        belief(vec![Decimal::ONE, Decimal::ZERO]),
        alpha,
        Decimal::ONE,
        Decimal::new(-356_674_943_938_i64, 12),
    )
}

fn auth_fixture() -> EntryAuth {
    EntryAuth {
        public_key: vec![0xAB; ED25519_PUBLIC_KEY_LEN],
        signature: vec![0xCD; ED25519_SIGNATURE_LEN],
    }
}

#[test]
fn belief_vector_roundtrip_is_lossless() {
    let cases: Vec<Vec<Decimal>> = vec![
        vec![Decimal::new(7, 1), Decimal::new(3, 1)],
        vec![Decimal::ONE, Decimal::ZERO],
        vec![Decimal::ZERO, Decimal::ONE],
        vec![Decimal::new(5, 1), Decimal::new(5, 1)],
        vec![
            Decimal::new(34, 2),
            Decimal::new(33, 2),
            Decimal::new(33, 2),
        ],
        // scale 18：10^18 定点可精确表示的最大精度
        vec![
            Decimal::new(123_456_789_012_345_678_i64, 18),
            Decimal::new(876_543_210_987_654_322_i64, 18),
        ],
    ];
    for allocations in cases {
        let original = belief(allocations);
        let vector = belief_to_vector(&original).unwrap();
        assert_eq!(vector.version, BELIEF_VECTOR_VERSION);
        assert_eq!(vector_to_belief(&vector).unwrap(), original);
    }
}

#[test]
fn belief_vector_wire_encoding_is_ten_pow_eighteen_fixed_point() {
    let vector = belief_to_vector(&belief(vec![Decimal::new(7, 1), Decimal::new(3, 1)])).unwrap();
    assert_eq!(
        vector.values,
        vec![700_000_000_000_000_000_u64, 300_000_000_000_000_000]
    );
    let one_hot = belief_to_vector(&belief(vec![Decimal::ONE])).unwrap();
    assert_eq!(one_hot.values, vec![FIXED_POINT_SCALE]);
}

#[test]
fn ledger_entry_roundtrip_is_lossless_with_and_without_auth() {
    let entry = sample_entry(Decimal::from(2_u64));

    // 带认证：域 → 契约 → 域，逐字段无损
    let proto = ledger_entry_to_proto(&entry, Some(&auth_fixture())).unwrap();
    assert_eq!(proto.seq, 42);
    assert_eq!(proto.new_alpha, "2");
    assert_eq!(proto.score, "-0.356674943938");
    let back = ledger_entry_from_proto(&proto).unwrap();
    assert_eq!(back.entry, entry);
    assert_eq!(back.auth, Some(auth_fixture()));

    // 无认证（#45 落地前的合法形态）
    let proto = ledger_entry_to_proto(&entry, None).unwrap();
    assert!(proto.auth.is_none());
    let back = ledger_entry_from_proto(&proto).unwrap();
    assert_eq!(back.entry, entry);
    assert_eq!(back.auth, None);

    // 超 u64 量级的 alpha：证明带符号标量走十进制字符串而非定点整数
    let big = sample_entry(Decimal::from_i128_with_scale(20_000_000_000_000_000_000, 0));
    let proto = ledger_entry_to_proto(&big, None).unwrap();
    assert_eq!(proto.new_alpha, "20000000000000000000");
    assert_eq!(ledger_entry_from_proto(&proto).unwrap().entry, big);
}

#[test]
fn rejects_fixed_point_out_of_range() {
    for bad in [FIXED_POINT_SCALE + 1, u64::MAX] {
        let vector = BeliefVector {
            version: BELIEF_VECTOR_VERSION,
            values: vec![bad],
        };
        assert!(
            matches!(
                vector_to_belief(&vector),
                Err(ConversionError::OutOfRange(_))
            ),
            "value {bad} must be rejected"
        );
    }
}

#[test]
fn rejects_unnormalized_belief() {
    // 元素和分别为 1.1 与 0.8，均须拒绝
    let over = BeliefVector {
        version: BELIEF_VECTOR_VERSION,
        values: vec![550_000_000_000_000_000, 550_000_000_000_000_000],
    };
    let under = BeliefVector {
        version: BELIEF_VECTOR_VERSION,
        values: vec![400_000_000_000_000_000, 400_000_000_000_000_000],
    };
    for vector in [over, under] {
        assert!(matches!(
            vector_to_belief(&vector),
            Err(ConversionError::InvalidBelief(_))
        ));
    }
}

#[test]
fn rejects_unsupported_belief_vector_version() {
    for version in [0, 2] {
        let vector = BeliefVector {
            version,
            values: vec![FIXED_POINT_SCALE],
        };
        assert!(matches!(
            vector_to_belief(&vector),
            Err(ConversionError::UnsupportedVersion(got)) if got == version
        ));
    }
}

#[test]
fn rejects_inexact_fixed_point_allocation() {
    // scale 19：0.1234567890123456789 × 10^18 非整数，定点无法精确表示
    let original = belief(vec![
        Decimal::new(1_234_567_890_123_456_789_i64, 19),
        Decimal::new(8_765_432_109_876_543_211_i64, 19),
    ]);
    assert!(matches!(
        belief_to_vector(&original),
        Err(ConversionError::NotRepresentable(_))
    ));
}

#[test]
fn rejects_invalid_decimal_strings() {
    for bad in ["", "abc", "1.2.3", "--1", "1,5"] {
        assert!(
            matches!(
                decimal_from_string(bad),
                Err(ConversionError::InvalidDecimal(_))
            ),
            "input {bad:?} must be rejected"
        );
    }
}

#[test]
fn rejects_auth_length_mismatch() {
    let entry = sample_entry(Decimal::ONE);
    let mut short_key = auth_fixture();
    short_key.public_key.truncate(ED25519_PUBLIC_KEY_LEN - 1);
    assert!(matches!(
        ledger_entry_to_proto(&entry, Some(&short_key)),
        Err(ConversionError::InvalidAuthLength {
            field: "public_key",
            ..
        })
    ));
    let mut short_sig = auth_fixture();
    short_sig.signature.truncate(ED25519_SIGNATURE_LEN - 1);
    assert!(matches!(
        ledger_entry_to_proto(&entry, Some(&short_sig)),
        Err(ConversionError::InvalidAuthLength {
            field: "signature",
            ..
        })
    ));
}

#[test]
fn conversion_error_codes_match_unified_model() {
    let cases = [
        (
            ConversionError::UnsupportedVersion(2),
            ErrorCode::InvalidArgument,
        ),
        (
            ConversionError::OutOfRange("123".into()),
            ErrorCode::OutOfRange,
        ),
        (
            ConversionError::NotRepresentable("x".into()),
            ErrorCode::OutOfRange,
        ),
        (
            ConversionError::InvalidBelief("sum".into()),
            ErrorCode::InvalidArgument,
        ),
        (
            ConversionError::InvalidDecimal("abc".into()),
            ErrorCode::InvalidArgument,
        ),
        (
            ConversionError::InvalidAuthLength {
                field: "signature",
                len: 63,
            },
            ErrorCode::InvalidArgument,
        ),
        (
            ConversionError::MissingField("belief"),
            ErrorCode::InvalidArgument,
        ),
    ];
    for (err, code) in cases {
        assert_eq!(err.error_code(), code, "for {err:?}");
    }
}

#[test]
fn contract_artifacts_contain_no_f64() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let protos = [
        format!("{manifest}/proto/v1/common.proto"),
        format!("{manifest}/proto/v1/node_verifier.proto"),
        format!("{manifest}/proto/v1/peer.proto"),
    ];
    for path in &protos {
        let content = std::fs::read_to_string(path).unwrap();
        assert!(!content.contains("double"), "{path} 禁止浮点字段类型");
        assert!(!content.contains("float"), "{path} 禁止浮点字段类型");
    }
    let openapi = std::fs::read_to_string(format!("{manifest}/openapi/v1.yaml")).unwrap();
    assert!(!openapi.contains("type: number"), "OpenAPI 禁止数值型标量");
    assert!(!openapi.contains("format: double"));
    assert!(!openapi.contains("format: float"));
}

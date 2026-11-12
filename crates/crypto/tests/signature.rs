//! ed25519 后端集成测试（std 环境；#45）

#![allow(clippy::unwrap_used, clippy::expect_used)]
use sies_crypto::{Ed25519Backend, Ed25519SecretKey, SignatureError, Signer, Verifier};

const SEED: [u8; 32] = [7; 32];
const MESSAGE: &[u8] = b"sies ed25519 contract message";

fn key() -> Ed25519SecretKey {
    Ed25519SecretKey::from_bytes(&SEED)
}

#[test]
fn sign_then_verify_roundtrip() {
    let secret = key();
    let signature = secret.sign_bytes(MESSAGE);
    assert!(Ed25519Backend::verify(&secret.public_key_bytes(), MESSAGE, &signature).is_ok());
    // 同路径走 trait 抽象（确保 ADR-6 的零成本层可被泛型复用）
    let via_trait = Ed25519Backend::sign(&secret, MESSAGE);
    assert_eq!(via_trait.as_ref(), signature.as_ref());
    assert_eq!(
        Ed25519Backend::public_key(&secret).as_ref(),
        &secret.public_key_bytes()
    );
}

#[test]
fn tampered_message_is_rejected() {
    let secret = key();
    let signature = secret.sign_bytes(MESSAGE);
    let mut tampered = MESSAGE.to_vec();
    tampered[0] ^= 0x01;
    let err =
        Ed25519Backend::verify(&secret.public_key_bytes(), &tampered, &signature).unwrap_err();
    assert_eq!(err, SignatureError::VerificationFailed);
}

#[test]
fn malformed_public_key_length_is_rejected() {
    let secret = key();
    let signature = secret.sign_bytes(MESSAGE);
    let err =
        Ed25519Backend::verify(&secret.public_key_bytes()[..31], MESSAGE, &signature).unwrap_err();
    assert_eq!(err, SignatureError::MalformedPublicKey { got: 31 });
}

#[test]
fn malformed_signature_length_is_rejected() {
    let secret = key();
    let signature = secret.sign_bytes(MESSAGE);
    let err =
        Ed25519Backend::verify(&secret.public_key_bytes(), MESSAGE, &signature[..63]).unwrap_err();
    assert_eq!(err, SignatureError::MalformedSignature { got: 63 });
}

#[test]
fn signature_is_deterministic_across_key_instances() {
    // RFC 8032 确定性 nonce：同种子同消息 → 同签名（跨平台比特一致，ADR #2 口径）
    let a = Ed25519SecretKey::from_bytes(&SEED).sign_bytes(MESSAGE);
    let b = Ed25519SecretKey::from_bytes(&SEED).sign_bytes(MESSAGE);
    assert_eq!(a, b);
}

#[test]
fn secret_key_debug_output_is_redacted() {
    let debug = format!("{:?}", key());
    assert_eq!(debug, "Ed25519SecretKey(<redacted>)");
    assert!(!debug.contains(&hex::encode(SEED)));
}

#[test]
fn key_export_import_roundtrip() {
    let secret = key();
    let reimported = Ed25519SecretKey::from_bytes(&secret.to_bytes());
    assert_eq!(reimported.public_key_bytes(), secret.public_key_bytes());
}

#[test]
fn generated_key_is_nonzero() {
    assert_ne!(
        Ed25519SecretKey::generate()
            .expect("OS entropy available")
            .to_bytes(),
        [0u8; 32]
    );
}

#[test]
#[ignore = "吞吐基准按需运行：just verify-bench（正式 criterion 套件属 #59）"]
fn verify_throughput_baseline() {
    use std::time::Instant;
    const N: u128 = 2000;
    let secret = key();
    let public = secret.public_key_bytes();
    let signature = secret.sign_bytes(MESSAGE);
    let start = Instant::now();
    for _ in 0..N {
        Ed25519Backend::verify(&public, MESSAGE, &signature).unwrap();
    }
    let ns_per_op = start.elapsed().as_nanos() / N;
    println!(
        "ed25519 verify: {ns_per_op} ns/op（约 {} µs/op）",
        ns_per_op / 1000
    );
    // 基线口径：单次验签 < 2 ms（2_000_000 ns）；实际数字记录进 #45 commit
    assert!(
        ns_per_op < 2_000_000,
        "验签单次开销 {ns_per_op} ns 超过 2ms 基线"
    );
}

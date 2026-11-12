//! 密钥生成/导入工具（#45 验收配套）
//!
//! 用法：
//! ```sh
//! cargo run -p sies-crypto --example keygen --features keygen -- generate <path>
//! cargo run -p sies-crypto --example keygen --features keygen -- import  <path>
//! ```
//! 私钥文件为 64 个 hex 字符（unix 下权限 0600）；输出只含公钥与 blake2b 指纹，
//! 私钥永不打印（#45 验收：密钥不出现在日志）。

use clap::{Parser, Subcommand};
use sies_crypto::{Blake2bHasher, Ed25519SecretKey, Hasher};
use std::path::PathBuf;

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 生成新 ed25519 私钥并写入 hex 文件
    Generate { path: PathBuf },
    /// 导入既有私钥文件：校验格式并打印公钥与指纹
    Import { path: PathBuf },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Generate { path } => {
            let secret =
                Ed25519SecretKey::generate().map_err(|e| format!("key generation failed: {e}"))?;
            std::fs::write(&path, format!("{}\n", hex::encode(secret.to_bytes())))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            print_public(&secret);
            Ok(())
        }
        Command::Import { path } => {
            let content = std::fs::read_to_string(path)?;
            let bytes =
                hex::decode(content.trim()).map_err(|e| format!("invalid hex secret: {e}"))?;
            let seed: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| "私钥文件必须恰为 32 字节（64 个 hex 字符）")?;
            let secret = Ed25519SecretKey::from_bytes(&seed);
            print_public(&secret);
            Ok(())
        }
    }
}

/// 只打印公钥与指纹——私钥字节永不进入输出流
fn print_public(secret: &Ed25519SecretKey) {
    let public = secret.public_key_bytes();
    println!("public_key          = {}", hex::encode(public));
    println!(
        "fingerprint(blake2b) = {}",
        hex::encode(Blake2bHasher::hash(&public))
    );
}

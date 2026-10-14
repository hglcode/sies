use tracing_subscriber::{EnvFilter, fmt};

/// 初始化全局 tracing subscriber
/// - 开发环境: pretty 输出 + DEBUG 级别
/// - 生产环境: JSON 输出 + INFO 级别
/// - 可通过 RUST_LOG 环境变量覆盖
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    #[cfg(debug_assertions)]
    fmt().with_env_filter(filter).pretty().init();

    #[cfg(not(debug_assertions))]
    fmt()
        .with_env_filter(filter)
        .json()
        .flatten_event(true)
        .init();
}

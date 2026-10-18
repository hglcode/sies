# SIES Rust 生产级编码规范 (Coding Standards)

本文档定义了 SIES 项目的 Rust 编码标准。所有提交的代码必须遵守这些规范，以确保系统的安全性、可维护性和高性能。

## 1. 依赖管理 (Dependency Management)

### 1.1 强制禁用 Default Features

**规则**：在 `Cargo.toml` 中引入任何依赖时，**必须**设置 `default-features = false`，然后仅显式启用所需的 features。
**原因**：防止隐式引入不必要的 `std` 依赖、网络库或加密库，减少编译时间、二进制体积和攻击面。

```toml
# ❌ 错误：隐式引入 default features (可能包含 std, network 等)
serde = { workspace = true }
reqwest = "0.11"

# ✅ 正确：显式声明所需 features
serde = { workspace = true, default-features = false, features = ["derive", "alloc"] }
reqwest = { version = "0.11", default-features = false, features = ["rustls-tls", "json"] }
```

### 1.2 依赖引入审批

- **避免重复**：引入新 crate 前，必须确认 workspace 中没有功能重叠的 crate（例如：不要同时引入 `chrono` 和 `time`，统一使用一种；不要同时引入 `tokio` 和 `async-std`）。
- **最小化原则**：优先使用标准库，其次使用 workspace 已有的 crate。引入新依赖必须在 PR 描述中说明理由。

## 2. 架构与边界 (Architecture & Boundaries)

### 2.1 Core 纯净性 (The Pure Core)

`sies-core` 及所有领域逻辑 crate 必须保持**绝对纯净**。

- **禁止 Async**：不允许 `async fn`, `.await`, `tokio`, `futures`。
- **禁止 IO**：不允许 `std::fs`, `std::net`, `std::env`, 数据库驱动。
- **禁止系统时间**：不允许直接调用 `std::time::SystemTime` 或 `chrono::Utc::now()`。时间必须通过参数注入（如 `fn process(current_time: u64, ...)`）。
- **确定性**：相同的输入必须永远产生相同的输出。

### 2.2 依赖方向

依赖箭头必须**严格指向内部**：
`Binary/CLI` -> `Adapters/Gateway` -> `Application/UseCases` -> `Core/Domain`
严禁 Core 层反向依赖 Infra 层。

## 3. 错误处理 (Error Handling)

### 3.1 错误库选择

- **Core / Library crates**：使用 `thiserror`。定义明确的、结构化的枚举错误。
- **Binary / App crates**：使用 `anyhow` 或自定义的顶层聚合错误，方便快速组合和打印上下文。

### 3.2 严禁 Panic

- **生产代码中禁止 `.unwrap()` 和 `.expect()`**。
- 仅在以下情况允许：
  1. 单元测试代码中。
  2. 在编译期或启动阶段（如 `lazy_static` 初始化正则表达式），且有明确的注释证明其绝对不会失败。
- 对于 `Option` 和 `Result`，使用 `?` 运算符、`map`/`and_then` 或 `unwrap_or_default` 进行安全解包。

## 4. 类型系统与 API 设计 (Type System & API)

### 4.1 原始类型偏执 (Primitive Obsession)

- 避免在公共 API 中传递裸的 `String`, `u64`, `i32` 表示业务概念。
- **使用 Newtype 模式**：
  ```rust
  // ❌ 错误
  fn transfer(from: String, to: String, amount: u64);

  // ✅ 正确
  pub struct AccountId(String);
  pub struct AssetId(String);
  pub struct Amount(Decimal);
  fn transfer(from: AccountId, to: AccountId, amount: Amount);
  ```

### 4.2 可见性控制

- 默认所有字段和模块为 `pub(crate)` 或私有。
- 只有在明确需要跨 crate 使用时才暴露 `pub`。
- 提供 Builder 模式或构造函数来初始化复杂结构体，避免直接暴露包含大量字段的 `pub struct`。

## 5. 并发与异步 (Concurrency & Async)

### 5.1 Async 边界

- 只有在涉及网络 IO、文件 IO 或高并发等待时才使用 `async`。
- CPU 密集型计算（如贝叶斯推断、矩阵运算）**必须**是同步的。如果在 async 上下文中调用，必须使用 `tokio::task::spawn_blocking`。

### 5.2 状态共享

- 优先使用消息传递（Channels）而非共享状态（Mutex/RwLock）。
- 如果必须使用锁，优先使用 `tokio::sync::Mutex`（跨 await 点时）或 `parking_lot::Mutex`（同步上下文中），并尽量缩小锁的粒度。

## 6. 工具链与格式化 (Tooling)

- **格式化**：提交前必须运行 `cargo fmt`。
- **Linting**：必须通过 `cargo clippy --all-targets --all-features -- -D warnings`。
- **CI 强制**：上述两项在 CI 中失败将直接阻断 PR 合并。

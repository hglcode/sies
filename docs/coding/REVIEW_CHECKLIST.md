# SIES Code Review 检查清单 (Review Checklist)

Reviewer 在 Approve PR 之前，必须对照此清单进行逐项检查。如果发现违反项，应 Request Changes 并要求作者修复。

## 📦 1. 依赖与构建 (Dependencies & Build)

- [ ] **Default Features**：新增的依赖是否都设置了 `default-features = false`？
- [ ] **重复依赖**：是否引入了与现有 workspace 依赖功能重叠的新 crate？
- [ ] **版本锁定**：如果是新增的外部依赖，版本号是否合理？（避免使用过于陈旧或未经生产验证的 0.x 版本）。

## 🏗️ 2. 架构与边界 (Architecture)

- [ ] **Core 纯净性**：`sies-core`（或其他 domain crate）中是否意外引入了 `async`, `tokio`, `std::fs`, `std::net`, 数据库或系统时间调用？
- [ ] **依赖方向**：是否存在底层模块反向依赖上层模块的情况？
- [ ] **接口隔离**：是否通过 Trait 对基础设施（如 DB, 外部 API）进行了抽象，以便于 Mock 测试？

## 🛡️ 3. 安全与健壮性 (Safety & Robustness)

- [ ] **无 Panic**：生产代码路径中是否清除了所有的 `.unwrap()` 和 `.expect()`？
- [ ] **错误处理**：错误是否被正确传播（使用 `?`）？是否吞掉了错误（空的 `catch` 或 `if let Err(_) = ...` 且不记录日志）？
- [ ] **输入验证**：来自外部（网络、文件、用户输入）的数据在反序列化后，是否进行了业务逻辑层面的边界校验？
- [ ] **敏感信息**：日志或错误信息中是否意外打印了密码、Token、私钥或用户 PII（个人身份信息）？

## 🧠 4. 并发与性能 (Concurrency & Performance)

- [ ] **Async 阻塞**：是否在 `async fn` 中执行了 CPU 密集型计算或同步阻塞 IO？（应使用 `spawn_blocking` 或重构为同步）。
- [ ] **锁的粒度**：`Mutex`/`RwLock` 的作用域是否足够小？是否存在死锁风险（如嵌套锁）？
- [ ] **内存分配**：在热路径（Hot path）中是否存在不必要的 `Vec` 扩容、`String` 拷贝或 `Box` 分配？

## 🧪 5. 测试与可观测性 (Testing & Observability)

- [ ] **测试覆盖**：新增的核心逻辑是否有对应的单元测试？
- [ ] **边界测试**：是否测试了空值、极大值、极小值、并发竞争等边界情况？
- [ ] **日志质量**：关键状态流转是否有 `tracing` 日志？日志级别（INFO/WARN/ERROR）是否准确？是否使用了结构化字段（如 `info!(user_id = %id, "login success")`）而不是字符串拼接？

## 📝 6. 代码风格与可读性 (Style & Readability)

- [ ] **命名规范**：变量、函数、结构体命名是否清晰表达了业务意图？（避免 `data`, `info`, `temp` 等模糊命名）。
- [ ] **Newtype 模式**：是否避免了"原始类型偏执"？（例如用 `UserId(u64)` 代替裸 `u64`）。
- [ ] **注释**：对于复杂的算法（如贝叶斯更新公式）或反直觉的业务规则，是否有清晰的文档注释（`///`）解释 **Why**，而不仅仅是 **What**？
- [ ] **CI 状态**：`cargo fmt`, `cargo clippy`, `cargo test` 是否全部在 CI 中亮绿灯？

# SIES 技术选型与依赖决策文档 (Tech Stack & Dependency Decisions)

**状态**: 活跃 (Active)
**最后更新**: 2026-10-13
**适用范围**: sies workspace 所有 crates (core, storage, chain, api)

## 1. 核心原则 (Guiding Principles)

在 SIES 项目的工程实践中，我们遵循以下技术选型原则：

1. **不重复造轮子 (Buy/Integrate over Build)**：对于通用的基础设施（网络、存储引擎、密码学原语、序列化），优先采用业界成熟、经过生产验证的开源方案。
2. **聚焦核心壁垒 (Focus on Core Differentiators)**：将研发精力集中在 SIES 的核心差异化逻辑上（如：贝叶斯权重演化、蚁群算法编排、特定领域的语义模型）。
3. **安全与审计优先 (Security First)**：涉及密码学和共识的模块，必须选择经过安全审计或有长期主网运行历史的 Crate。
4. **最小依赖与可控性 (Minimal & Controlled Dependencies)**：避免引入过于庞大或维护状态不明的框架，优先选择 `no_std` 兼容、模块化设计良好的 Rust crate。

---

## 2. 核心自研边界 (What We MUST Build Ourselves)

以下模块是 SIES 的核心护城河与业务逻辑载体，**严禁**使用通用开源框架直接替代，必须根据领域模型自行设计与实现：

| 模块 | 描述 | 为什么自研 |
| :--- | :--- | :--- |
| **WeightEvolutionEngine** | 贝叶斯更新与蚁群算法的编排引擎 | 核心算法创新，通用框架无法表达特定的业务演化逻辑 |
| **DecisionEvent / Domain Model** | 决策事件语义模型与状态机 | 强业务绑定，决定了系统的输入输出契约 |
| **Belief & Scoring Strategies** | 置信度计算与评分策略 | 属于算法研究的直接产出，需要高度定制化 |

---

## 3. 模块级技术选型 (Module-Level Tech Stack)

### 3.1 Chain & Crypto (链与密码学层)

**目标**：提供安全、高性能的密码学原语与状态证明。

| 功能 | 最终选择 | 候选/排除方案 | 决策理由 (Rationale) |
| :--- | :--- | :--- | :--- |
| **Merkle Tree** | `sparse-merkle-tree` (Nervos) | 自研 / `jellyfish-merkle` | CKB 主网验证 5+ 年；API 简洁，支持 `no_std`，Hasher 可插拔。 |
| **Hash 函数** | `blake2b-rs` / `blake3` | SHA-256 (`sha2`) / 自研 | Blake2b 是 SMT 默认且性能优异；若需极致吞吐可选 `blake3`。避免自研。 |
| **数字签名** | `ed25519-dalek` | `ring` / `secp256k1` | Ed25519 黄金标准，纯 Rust 实现，安全性高。除非需兼容以太坊，否则不选 secp256k1。 |

### 3.2 Storage (存储与持久化层)

**目标**：提供高性能的 KV 存储、状态序列化与持久化。

| 功能 | 最终选择 | 候选/排除方案 | 决策理由 (Rationale) |
| :--- | :--- | :--- | :--- |
| **KV 存储引擎** | `sled` (首选) / `rocksdb` | 自研 B-Tree / `redb` | `sled` 纯 Rust、无 C++ 绑定、嵌入式友好；若数据量极大需调优可切 `rocksdb`。 |
| **二进制序列化** | `postcard` / `rkyv` | `bincode` / `serde_json` | `postcard` 极致紧凑且 `no_std` 友好；`rkyv` 提供零拷贝反序列化，适合高频状态读取。 |
| **状态后端适配** | 自研 Trait + SMT 适配 | - | 定义 `StateStore` trait，底层挂载 SMT 与 KV 引擎。 |

### 3.3 Core (核心算法与工具层)

**目标**：提供数学计算、精度保证与基础工具链。

| 功能 | 最终选择 | 候选/排除方案 | 决策理由 (Rationale) |
| :--- | :--- | :--- | :--- |
| **统计与分布** | `statrs` | `nalgebra` (仅矩阵) | 提供完整的概率分布、采样、假设检验，契合贝叶斯计算需求。 |
| **高精度计算** | `rust_decimal` | `f64` / `bigdecimal` | 避免浮点数精度陷阱，金融/权重计算必备，API 友好。 |
| **错误处理** | `thiserror` + `anyhow` | 自定义 String 错误 | 库(crate)用 `thiserror` 定义强类型错误，应用/测试用 `anyhow` 包装上下文。 |

### 3.4 API (网络与接口层)

**目标**：提供高性能、标准化的 RPC 与 REST 接口。

| 功能 | 最终选择 | 候选/排除方案 | 决策理由 (Rationale) |
| :--- | :--- | :--- | :--- |
| **REST/HTTP** | `axum` | `actix-web` / `rocket` | Tokio 官方推荐，Tower 中间件生态完善，异步体验最佳。 |
| **gRPC** | `tonic` | `grpc-rs` | Rust gRPC 事实标准，基于 `prost` 代码生成，性能优异。 |
| **OpenAPI/Swagger** | `utoipa` | `paperclip` | 从 Rust 类型自动生成 OpenAPI spec，与 `axum` 无缝集成。 |
| **JSON-RPC** | `jsonrpsee` | 自研 / `jsonrpc-core` | Parity 出品，支持 WS/HTTP 双协议，Substrate 生态标配。 |

### 3.5 Infrastructure (基础设施)

**目标**：提供可观测性、配置管理与异步运行时。

| 功能 | 最终选择 | 候选/排除方案 | 决策理由 (Rationale) |
| :--- | :--- | :--- | :--- |
| **异步运行时** | `tokio` | `async-std` / `smol` | 绝对的行业标准，生态最繁荣。 |
| **日志与追踪** | `tracing` | `log` + `env_logger` | 结构化日志，完美支持异步上下文跨度 (Span)，可观测性基石。 |
| **配置管理** | `figment` | `config-rs` / 纯 TOML | 支持 TOML/YAML/ENV 多源合并与覆盖，类型安全。 |

---

## 4. 依赖管理与安全策略 (Dependency & Security Policy)

为了防止依赖腐化和供应链攻击，项目强制执行以下策略：

1. **安全审计**：
   - CI 流水线中必须包含 `cargo audit`。
   - 引入新依赖前，需检查其维护状态、下载量及是否有已知 CVE。
2. **依赖瘦身**：
   - 定期运行 `cargo tree --depth 2` 检查依赖膨胀。
   - 优先选择支持 `default-features = false` 的 crate，按需开启 feature。
3. **版本锁定**：
   - 库 crate (如 `sies-core`) 尽量使用宽泛的版本范围 (如 `"1"`)。
   - 二进制/应用 crate 必须提交 `Cargo.lock`。
4. **Deny 策略**：
   - 引入 `cargo-deny` 工具，在 CI 中禁止引入 GPL 等传染性开源协议的依赖，确保商业/开源友好 (MIT/Apache-2.0 优先)。

---

## 5. 决策记录 (Decision Log)

| 日期 | 决策 | 上下文 |
| :--- | :--- | :--- |
| 2026-10-13 | 采用 `sparse-merkle-tree` 替代自研 SMT | 任务 #7 启动时评估。自研密码学树结构风险高、周期长，Nervos 方案已在 CKB 主网验证，符合"不造轮子"原则。 |
| 2026-10-13 | 确立 `axum` + `tonic` 为 API 层基座 | 任务 #11 预研。统一异步生态，利用 Tower 中间件复用鉴权/限流逻辑。 |

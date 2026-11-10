# SIES 架构设计文档 V2.1

## 一、 系统愿景与核心理念

SIES
是一个**面向广义智能实体的去中心化演化基础设施**。通过链上不可篡改的行为轨迹与可插拔的演化引擎，让任何智能实体（人类个体/群体、AI
模型、AI Agent）的“能力权重”根据其历史行为反馈进行客观、透明、动态的演化。

## 核心设计哲学：

1. **广义智能实体 (Generalized Agent)**：协议不区分人类与机器，所有参与者统一抽象为具备身份、行为、信念与信誉的智能实体。
2. **不信任，只验证 (Trustless & Verifiable)**：所有行为输入、结果反馈和权重计算逻辑均可公开验证，任何人可审计其公平性。
3. **能力证明 (Proof of Competence)**：权重不依赖于初始财富或身份类型，而是基于历史行为质量的动态演化。
4. **绝对确定性 (Absolute Determinism)**：核心引擎严禁使用原生浮点数（f32/f64），所有数学运算基于高精度定点数与确定性泰勒展开，确保跨平台比特级一致。

---

## 二、 核心算法模型设计

权重的演化融合了**贝叶斯更新（微观概率修正）** 与**蚁群信息素机制（宏观时间衰减与正反馈）**。

### 核心抽象：信念-结果模型 (Belief-Outcome Model)

在底层数学模型中，投票、预测、AI
推理等行为被统一抽象为：**对不确定事件表达信念（Belief），并接受事后结果验证（Outcome）**。

- **Belief (信念)**：智能体对 $N$ 个互斥选项分配的归一化权重向量（概率分布）。
- **Outcome (结果)**：事件结算时的真实状态（Ground Truth），通常表示为 One-hot 向量。
- **Scoring (评估)**：通过严格适当评分规则（Strictly Proper Scoring Rules，如 Brier Score）计算
  Belief 与 Outcome 之间的偏差得分。

### 1. 串联演化引擎 (Swarm Evolution Engine)

在 `sies-core::engine` 中，单轮演化被严格编排为两个串联阶段：

1. **Phase 1: 贝叶斯个体学习 (Bayesian Updating)**
   将智能体的“能力”视为隐藏的 Beta 分布 $Beta(\alpha, \beta)$。
    - 根据 Scoring Rule 计算归一化得分 $S_{norm} \in [0, 1]$。
    - 更新伪计数：$\alpha \leftarrow \alpha + S_{norm} \cdot \eta$，$\beta \leftarrow \beta + (1 - S_{norm}) \cdot \eta$（$\eta$
      为学习率）。
2. **Phase 2: 群体信息素演化 (Swarm Pheromone Evolve)**
   在贝叶斯更新后的权重基础上，执行群体层面的信息素蒸发与沉积。
    - **全局蒸发**：$W' = W \cdot (1 - \rho)$，$\rho$ 为蒸发率。
    - **信息素沉积**：$W'' = W' + Q \cdot \text{accuracy}$，$Q$ 为沉积强度，accuracy 为 Belief 与
      Outcome 的点积。
    - **状态反解**：将最终权重 $W''$ 映射回新的 $(\alpha, \beta)$ 参数。

### 2. 时间衰减机制 (Pheromone Decay)

对于长时间不活跃的实体，系统通过 `PheromoneDecay` 模块执行独立的时间衰减：
$$W_{new} = W_{old} \cdot (1 - \rho)^{t / t_{half}}$$
为确保跨平台确定性，该公式中的 $ln$ 和 $exp$
运算通过**固定项数的泰勒级数**（20项）实现，完全规避了标准库的浮点超越函数。

---

## 三、 工程架构与模块划分 (Rust Workspace)

系统采用 Rust Workspace 组织，严格遵循依赖倒置原则。当前实际包含以下 Crate：

### 1. 核心算法层 (`crates/core`)

纯数学与逻辑层，**零外部 I/O 依赖**。

- `bayesian`: `BetaState` 与 `BayesianUpdater`。包含 40
  字节定长序列化（`to_raw_bytes`/`from_raw_bytes`）以绕过 Postcard 对 Decimal 的不兼容。
- `belief`: `Belief` 结构，强制归一化校验（容差 $10^{-9}$）。
- `scoring`: `Scorer`，支持 Brier Score（已实现）与 Log
  Score（_注：当前 Log Score 使用 f64 占位，为已知技术债_）。
- `decay`: `PheromoneDecay`，确定性泰勒级数数学近似。
- `swarm`: `SwarmUpdater`，信息素蒸发与沉积。
- `engine`: `SwarmEvolutionEngine`，编排上述模块。

### 2. 存储与状态层 (`crates/storage`, `crates/chain`)

- **`sies-storage`**：定义 `StateStore` 和 `Ledger` Trait。包含 `FileLedger` 实现（Append-only
  文件 + CRC32 校验 + 独立索引文件 `ledger.idx`）。定义了 `LedgerEntry` 与定点数表示的
  `RawLedgerEntry`（$10^{18}$ 精度）。
- **`sies-chain`**：
  - `smt`: `SmtStateStore`，基于 `sparse-merkle-tree` 和 `blake2b-rs`（带自定义
    personalization）实现的状态存储。
  - `rebuilder`: `Rebuilder`，从 Ledger 确定性批量重建 SMT 状态，支持 Root Hash 验证。

### 3. 服务与网络层 (`bins/*`, `crates/rpc`, `crates/p2p`)

- **`sies-node`**：核心全节点（骨架阶段）。
- **`sies-verifier`**：ZK/欺诈证明验证节点（骨架阶段）。
- **`sies-gateway`**：对外 API 网关（骨架阶段）。
- **`sies-rpc`**：基于 `tonic` 和 `prost` 的 gRPC 接口定义（`proto/v1/*`）。
- **`sies-p2p`**：libp2p 网络层封装（占位）。

### 4. 基础设施与测试 (`crates/infra`, `crates/e2e`)

- **`sies-infra`**：基于 `figment` 的多源配置加载（TOML + 环境变量），基于 `tracing-subscriber`
  的日志初始化。
- **`sies-e2e`**：E2E 测试框架。提供 `TestNode` Harness（封装临时目录、Ledger 与 SMT
  的冷启动/重启）、Fixture 生成器与语义化断言（`assert_smt_root_eq` 等）。

---

## 四、 链上数据模型与序列化设计

### 1. 核心数据结构

| 数据结构           | 描述                             | 存储/序列化方式                                                                                                  |
| :----------------- | :------------------------------- | :--------------------------------------------------------------------------------------------------------------- |
| **BetaState**      | 个体的信誉状态 $(\alpha, \beta)$ | 40 字节定长布局：`[alpha_mantissa: i128 LE] [alpha_scale: u32 LE] [beta_mantissa: i128 LE] [beta_scale: u32 LE]` |
| **LedgerEntry**    | 单次状态转换的不可变记录         | 包含 seq, round, user_id, belief/outcome allocations, new_alpha, new_beta, score                                 |
| **RawLedgerEntry** | 磁盘/网络传输格式                | 所有 `Decimal` 字段映射为 $10^{18}$ 定点数 `u64`，使用 `postcard` 序列化                                         |
| **SMT Node**       | 稀疏默克尔树节点                 | Key: `blake2b(user_id)` (personalized)；Value: `BetaState` 的 40 字节 raw bytes                                  |

### 2. FileLedger 磁盘格式

- **数据文件 (`ledger.dat`)**：`[4B payload_len LE] [payload_len B postcard-encoded RawLedgerEntry] [4B CRC32 LE]`
- **索引文件 (`ledger.idx`)**：`[8B file_offset LE] × N`，用于 $O(1)$ 随机读取。

---

## 五、 系统安全与确定性控制 (Determinism & Security)

### 1. 绝对确定性原则 (ADR #2)

在分布式共识环境中，状态分叉是致命的。SIES 强制实施以下规则：

- **禁止原生浮点数**：核心引擎严禁使用 `f32`/`f64`。所有概率、权重计算使用 `rust_decimal`。
- **确定性超越函数**：`decay.rs` 中的 $ln$ 和 $exp$ 必须使用固定项数（20项）的泰勒级数实现。
- _已知技术债_：`scoring.rs` 中的 Log Score 当前使用 `f64::ln()`
  作为占位符，需在后续版本替换为确定性近似实现。

### 2. 防篡改与可验证性

- **Append-Only Ledger**：`FileLedger` 仅支持追加，配合 CRC32 防止静默数据损坏。
- **确定性重放 (Deterministic Replay)**：`Rebuilder` 可从创世区块逐条重放
  Ledger，重建出与运行时完全一致的 SMT Root Hash。
- **密码学隔离**：SMT 的 Key 生成（`sies-state-key`）与内部节点哈希（`sies-smt-hash!`）使用不同的
  Blake2b personalization，防止跨域碰撞攻击。

---

## 六、 业务流转机制 (Workflow)

1. **行为提交**：智能体提交 Belief（签名），Executor 验证归一化后暂存。
2. **结果注入**：Oracle 或自动化脚本注入 Outcome。
3. **引擎演化**：`SwarmEvolutionEngine::step()` 执行贝叶斯更新与 Swarm 演化，输出新的 `BetaState`。
4. **状态持久化**：
    - 将转换记录写入 `FileLedger`（分配 seq，计算 CRC）。
    - 将新的 `BetaState` 写入 `SmtStateStore`（更新 SMT 叶子节点）。
5. **共识与验证**：节点广播新的 SMT Root Hash。验证节点（Verifier）可通过重放 Ledger 独立计算 Root
   Hash 进行对账。

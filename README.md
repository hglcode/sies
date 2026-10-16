# SIES — 群体智能演化系统 (Swarm Intelligence Evolution System)

> 面向一切可演化智能实体的去中心化评分与协同演化基础设施
> 覆盖人类个体/群体、AI 模型、AI Agent 等异构智能体

## 项目概述

SIES 是一个用 Rust 构建的**通用智能演化基础设施**。其核心目标是为**一切拥有智能演化能力的实体**提供可信评分、信念传播与协同演化的底层协议。

SIES 所定义的「智能体（Agent）」是广义的，包括但不限于：

- **人类个体** — 以 DID / 钱包签名作为身份锚点，行为数据上链参与评分
- **人类群体** — 由个体组成的协作组织，具备群体级信念与声誉演化
- **AI 模型** — 大语言模型、专用模型等，通过调用记录与输出质量评估参与演化
- **AI Agent** — 自主决策的软件智能体，原生支持链上行为验证与状态转换

系统融合了贝叶斯推理、蚁群优化和稀疏 Merkle Tree（SMT）状态证明，为上述**异构智能体**之间的跨类型协作、竞争与共同演化提供可验证、抗篡改的统一框架。

## 目录结构

```txt
sies/
├── Cargo.toml              # Workspace 根配置
├── bins/                   # 可执行二进制
│   ├── node/               # sies-node: 核心节点（共识 + 引擎 + 存储）
│   ├── verifier/           # sies-verifier: 轻量验证节点
│   └── gateway/            # sies-gateway: RPC/REST 网关
├── conf/                   # 默认配置文件
│   ├── node.toml
│   ├── verifier.toml
│   └── gateway.toml
├── crates/                 # 核心库
│   ├── core/               # [sies-core] 贝叶斯引擎、蚁群算法、评分规则、信念衰减
│   ├── chain/              # [sies-chain] SMT 状态树、rebuilder、链上状态转换
│   ├── storage/            # [sies-storage] StateStore trait + FileLedger 持久化
│   ├── consensus/          # [sies-consensus] 共识协议抽象
│   ├── executor/           # [sies-executor] 交易/指令执行器
│   ├── crypto/             # [sies-crypto] 密码学原语
│   ├── p2p/                # [sies-p2p] 点对点网络层
│   ├── rpc/                # [sies-rpc] gRPC 接口定义 (protobuf) + 服务端
│   ├── infra/              # [sies-infra] 配置加载、tracing 初始化
│   └── e2e/                # [sies-e2e] E2E 测试框架（harness / fixtures / asserts）
└── docs/                   # 文档
    ├── design/             # 架构设计、技术栈、数学模型
    ├── whitepaper.md       # 白皮书
    └── README.md           # 文档索引
```

## 快速开始

### 前置要求

- Rust ≥ 1.85 (stable)
- protobuf compiler (`protoc`)

### 构建

```bash
cargo build --workspace
```

### 运行节点

```bash
cargo run -p sies-node -- --config conf/node.toml
```

### 运行测试

```bash
# 单元测试 + 集成测试
cargo test --workspace

# E2E 冒烟测试
cargo test -p sies-e2e --test smoke
```

## Crate 说明

| Crate            | 描述                                                         |
| ---------------- | ------------------------------------------------------------ |
| `sies-core`      | 核心算法：贝叶斯信念更新、蚁群信息素扩散、评分聚合、信念衰减 |
| `sies-chain`     | 稀疏 Merkle Tree (SMT)、状态重建器 (rebuilder)、状态根计算   |
| `sies-storage`   | `StateStore` trait 抽象 + `FileLedger` 文件级持久化实现      |
| `sies-consensus` | 共识协议接口与实现                                           |
| `sies-executor`  | 指令/交易执行引擎                                            |
| `sies-crypto`    | 哈希、签名等密码学工具                                       |
| `sies-p2p`       | libp2p 网络层封装                                            |
| `sies-rpc`       | gRPC 服务定义（proto v1）与服务端实现                        |
| `sies-infra`     | TOML 配置加载、tracing-subscriber 日志初始化                 |
| `sies-e2e`       | E2E 测试框架：TestNode harness、fixture 生成、断言工具集     |

> **关于 Agent 的定义**：SIES 协议设计上的 Agent 是广义智能实体。当前代码实现以软件 AI Agent 为主要原生载体，但底层协议已预留人类身份锚点（DID）、群体聚合、AI 模型评估接口等扩展点，确保未来可无缝接入人类与各类 AI 模型。

## E2E 测试

E2E 测试框架位于 `crates/e2e/`，提供：

- **TestNode Harness** — 在临时目录中启动完整节点实例
- **Fixtures** — 预置测试数据生成器（agents、transactions、blocks）
- **Asserts** — 语义化断言（状态根匹配、余额校验、SMT 一致性）

```bash
# 运行全部冒烟测试
cargo test -p sies-e2e --test smoke

# 运行特定测试
cargo test -p sies-e2e --test smoke cold_start
```

## 文档

- [架构设计](docs/design/01-arch.md)
- [技术栈选型](docs/design/02-tech_stack.md)
- [数学模型](docs/design/03-mathematical-models.md)
- [白皮书](docs/whitepaper.md)

## License

TBD

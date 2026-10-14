# SIES(决策权重演化系统)

## 目录结构

```txt
sies/
├── Cargo.toml          # [Workspace 根配置] 统一管理依赖和成员
├── README.md
├── crates/
│   ├── core/           # [sies-core] 核心算法：贝叶斯、蚁群、评分规则
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── storage/        # [sies-storage] 存储抽象层：StateStore Trait
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── chain/          # [sies-chain] 链上逻辑：Merkle树、状态转换、ZK接口
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   └── api/            # [sies-api] 接口层：RPC、CLI、REST
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
├── docs/               # 文档目录
│   ├── coding/
│   ├── design/
│   │   └── arch.md
│   └── testing/
└── tests/              # 跨 crate 的集成测试 (Integration Tests)
```

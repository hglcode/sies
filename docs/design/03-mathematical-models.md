# 数学模型与可插拔演化架构

## 3.1 设计哲学：算法无关的演化协议

SIES 的核心价值不在于内置了某一种最优的群体决策算法，而在于提供了一个**严格定义输入输出边界、支持任意算法热插拔的演化容器**。不同的应用场景（舆论、选举、预测）对“正确”的定义和演化动力学完全不同，因此系统必须在业务实现和设计模式上保持绝对的算法中立。

我们将系统拆分为两个正交的维度：

- **State Protocol (状态协议)**: 定义 Agent 信念和信誉的数据结构契约（如概率分布、标量权重）。
- **Evolution Strategy (演化策略)**: 定义状态如何随时间、事件和群体交互而变化的纯函数逻辑。

## 3.2 可插拔打分机制 (Pluggable Scoring Rules)

打分机制决定了“事实（Outcome）”如何转化为“个体反馈信号”。系统不假设任何特定的评分标准。

### 3.2.1 策略接口契约

任何打分算法必须满足**严格适当性（Strictly Proper）** 或显式声明为非适当评分，以防止 Agent 通过策略性撒谎获利。

```rust
/// 打分策略接口 (对应 crates/core/src/scoring.rs)
pub trait ScoringRule: Send + Sync {
    /// 预测值域类型 (如 f64, Decimal, Categorical)
    type Prediction;
    /// 结果值域类型
    type Outcome;
    /// 得分类型 (通常为 Decimal，越高越好或越低越好需文档注明)
    type Score;

    fn score(&self, prediction: &Self::Prediction, outcome: &Self::Outcome) -> Self::Score;

    /// 声明该评分规则是否为严格适当的
    fn is_strictly_proper(&self) -> bool;
}
```

### 3.2.2 内置参考实现 (Batteries Included)

白皮书应列出首批支持的策略，作为生态基准：

- `BrierScore`: 二次损失，适用于二元/多元概率校准。
- `LogarithmicScore`: 对数损失，对极端自信的错误惩罚极大，适用于高风险预测。
- `SphericalScore`: 球面评分，对概率向量的模长敏感。
- `CustomWeightedScore`: 允许业务方传入自定义权重矩阵的非标准评分。

## 3.3 可插拔信誉更新 (Pluggable Belief/Reputation Update)

信誉更新定义了“打分结果”如何修改 Agent 的内部状态。这是贝叶斯学习与强化学习的交汇点。

### 3.3.1 策略接口契约

更新器必须是**无状态的纯函数**，所有持久化状态由外部引擎管理。

```rust
/// 个体状态更新策略
pub trait BeliefUpdater: Send + Sync {
    type State;      // e.g., BetaState, GaussianState, ScalarWeight
    type Signal;     // e.g., Score from ScoringRule
    type Error;

    /// 根据反馈信号计算下一个状态
    /// 注意：此方法不应有副作用
    fn update(
        &self,
        current: &Self::State,
        signal: &Self::Signal
    ) -> Result<Self::State, Self::Error>;
}
```

### 3.3.2 演化动力学分类

白皮书需明确 SIES 支持的更新范式，供接入方选择：

- **Bayesian Conjugate Update**: 共轭先验更新（如 Beta-Binomial），计算高效，适合在线流式数据。
- **Gradient-Based Update**: 基于评分梯度的 SGD/Adam 更新，适合复杂非共轭模型。
- **Heuristic Decay Update**: 基于规则的衰减（如半衰期、滑动窗口），适合缺乏严格概率解释的声誉系统。
- **RLHF-Style Reward Shaping**: 引入折扣因子和资格迹（Eligibility Traces），适合延迟反馈场景。

## 3.4 可插拔群体聚合 (Pluggable Collective Aggregation)

_(替代原 AntColony 概念)_
群体聚合定义了多个 Agent 状态如何融合为系统级输出或相互影响。

### 3.4.1 策略接口契约

```rust
/// 群体聚合/社会学习策略
pub trait AggregationStrategy: Send + Sync {
    type AgentState;
    type GlobalContext; // 可选的全局上下文（如当前共识、网络拓扑）
    type Output;

    /// 聚合多个Agent状态
    /// 可以是简单的加权平均，也可以是复杂的社会学习扩散
    fn aggregate(
        &self,
        states: &[Self::AgentState],
        context: Option<&Self::GlobalContext>,
    ) -> Result<Self::Output, AggregationError>;
}
```

### 3.4.2 聚合范式谱系

- **Static Weighted Average**: 经典 Wisdom of Crowds，权重仅依赖历史信誉。
- **DeGroot / Friedkin-Johnsen Dynamics**: 线性社会学习，Agent 迭代采纳邻居意见，研究共识收敛条件。
- **Attention-Based Pooling**: 基于 Transformer 自注意力机制的动态权重分配，捕捉非线性相关性。
- **Robust Aggregation (Trimmed Mean / Median)**: 抗拜占庭/抗串谋聚合，自动剔除异常值。
- **Information Cascade Model**: 显式建模信息瀑布效应，防止早期错误信号锁定群体认知。

## 3.5 编排引擎：组合而非继承

`WeightEvolutionEngine` 不是具体算法的实现者，而是上述三种可插拔策略的**组合器（Combinator）**。

```rust
// 引擎通过泛型或动态分发持有策略引用
pub struct WeightEvolutionEngine<S, U, A>
where
    S: ScoringRule,
    U: BeliefUpdater,
    A: AggregationStrategy,
{
    scoring: Arc<S>,
    updater: Arc<U>,
    aggregation: Arc<A>,
    config: EvolutionConfig,
}
```

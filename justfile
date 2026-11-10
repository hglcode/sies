set shell := ["bash", "-cu"]

default:
    @just --list

# 覆盖率报告（lcov 输出到 .coverage/lcov.info）
cov:
    @if command -v cargo-llvm-cov >/dev/null 2>&1; then \
        mkdir -p .coverage && cargo llvm-cov --workspace --lcov --output-path .coverage/lcov.info; \
    else echo "⚠ cargo-llvm-cov 未安装，跳过覆盖率（cargo install cargo-llvm-cov）"; fi

# 格式化全部代码
fmt:
    cargo fmt --all && rumdl check --fix

# 格式化检查（不落盘，CI 等价检查）
fmt-check:
    cargo fmt --all -- --check

# fuzz 目标可编译检查（需 nightly + cargo-fuzz；长跑见 docs/testing/01 §6）
fzz:
    @if command -v cargo-fuzz >/dev/null 2>&1; then \
        cargo +nightly fuzz check; \
    else echo "⚠ cargo-fuzz 未安装，跳过（cargo install cargo-fuzz；需 nightly 工具链）"; fi

# 拼写检查（提交场景已由 prek typos 钩子覆盖；本命令供本地手动使用）
typ:
    @if command -v typos >/dev/null 2>&1; then \
        typos; \
    else echo "⚠ typos 未安装，跳过（cargo install typos-cli）"; fi

# lint 全量门禁：clippy 极严（-D warnings）+ 依赖审计 + 拼写
lnt:
    cargo clippy --workspace --all-targets -- -D warnings
    @if command -v cargo-deny >/dev/null 2>&1; then \
        cargo deny check --hide-inclusion-graph; \
    else echo "⚠ cargo-deny 未安装，跳过依赖审计（pre-push 将硬阻塞，请安装）"; fi
    @just typ

# 未使用依赖检查
mac:
    @if command -v cargo-machete >/dev/null 2>&1; then \
        cargo machete; \
    else echo "⚠ cargo-machete 未安装，跳过（cargo install cargo-machete）"; fi

# 全量测试：单元 + 集成（nextest 缺席回退 cargo test）+ 文档测试
# 注：cargo nextest 不执行 doctest，故 --doc 单独显式跑（勿并入回退分支重复执行）
tst:
    @if command -v cargo-nextest >/dev/null 2>&1; then \
        cargo nextest run --workspace; \
    else echo "⚠ nextest 未安装，回退 cargo test"; cargo test --workspace --quiet; fi
    cargo test --workspace --doc --quiet

# E2E 冒烟测试（任务 #26 原范围；CI test job 跑的是其超集 --workspace）
e2e-smoke:
    cargo nextest run -p sies-e2e --test smoke

# 形式化验证（kani）—— proof harness 以 cfg(kani) 隔离，常规构建零成本
kani:
    @if command -v cargo-kani >/dev/null 2>&1; then \
        cargo kani --quiet --workspace; \
    else echo "⚠ kani 未安装，跳过（安装方式见 docs/testing/01 §7.2）"; fi

# 安装/更新 git hooks（prek）
hooks:
    prek install -c prek.toml
    @echo "hooks 安装完成：pre-commit=fmt+clippy+typos+文件卫生+密钥/大文件防线，pre-push=test+deny"

# 本地全量门禁（等价未来 CI 的检查集合，推送前建议跑一次）
# 未安装的可选工具会提示并跳过——这是本地开发体验的取舍；
# 推送（pre-push）与发版（rls）门禁不降级。
ci: fmt-check lnt mac tst kani
    @echo "本地全量门禁通过"

# 发版门禁：ci 之上加 fuzz 目标编译检查与覆盖率报告
# 发版前另跑 fuzz 长跑：cargo +nightly fuzz run <target> -- -max_total_time=3600
rls: fmt-check lnt mac tst kani fzz cov
    @echo "发版门禁通过（kani/fuzz/覆盖率含于本命令；fuzz 长跑按 testing §6 手动执行）"

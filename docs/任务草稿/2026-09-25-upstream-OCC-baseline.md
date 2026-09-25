# 2026-09-25 上游 OCC 基线

**目标：** 不再改这条 fork 的 OCC 热路径。把 risechain/pevm `e94b0e3` 原样接成 OCC 计时基线，扫描表报告 1 核 `TPS_SEQ`、上游 `TPS_OCC`、本 fork `TPS_SF` 和 `TPS_ideal(C)`。

**约束：**

- 上游库 `src/` 不改。允许的例外只有 criterion bench 的 `PEVM_BENCH_CONCURRENCY`，以及为了和本仓库 `pevm` 共存而改的包名 / `use`。
- OCC 与 SEQ、SF 用同一段整块计时：交易列表和 block env 建好之后，包住引擎调用。
- 每个 round 新建引擎，没有同块热身。K 次，中位数加 bootstrap 95% CI。
- CPU 列表可配，默认 `128-255`。本机用 `0-3`。
- 不做 OCC 性能修复。

**完成标准：**

- `pevm_upstream` 能和 fork 一起编过，harness 的 wall `occ` 行走上游 `execute_revm_parallel`。
- 两个块上上游并行结果等于上游顺序结果。
- `scripts/soft0_percore_scan.sh` 的曲线把 `TPS_SEQ` 收成 1 核一个数。
- 本机 C=1/4/8 烟测有中位数和 CI，并写明宿主。C=8 在 4 核上是超订。

**步骤：**

1. 已完成：撤回未提交的 OCC 快路径改动。
2. 已完成：`crates/pevm_upstream` 的 `src/` 与 `e94b0e3` 字节相同。墙时 OCC 走它的 `execute_revm_parallel`。
3. 已完成：本机 C=1/4/8 烟测。C=8 超订。上游 par≡seq，两块 3/3。数字在 `docs/occ-regression-vs-upstream.md`。
4. 已完成：代码检视列出同样落在 SF `Vm::execute` 上的回退。没有百分比，没有修复。

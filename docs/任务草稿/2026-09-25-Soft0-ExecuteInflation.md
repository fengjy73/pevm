# Soft=0 执行膨胀测量

**目标：** 只加探针和脚本，不改调度 / Avoid。在本机做探针、列表调度和烟测；256 核曲线交给 ict21 上的 `scripts/soft0_percore_scan.sh`。主输出是 `TPS_ideal(C)`、`TPS_OCC(C)`、`TPS_SF(C)`。同时回答 A–G。

**约束：**

- 计时边界是 tx 列表和内存状态已经备好之后的引擎入口。`seq` = `execute_revm_sequential`。`occ` = `execute_revm_parallel` / Occ。`sf` = `execute_revm_parallel` / SpecFence / `run_sf_block`。SF 不走 OCC 工人环，也不走 `Pevm::execute` 的 gas / `n_tx < workers` 回退。
- 没有不计时热身。每个 timed 轮都是新 `Pevm`。K≥10。中位 + bootstrap 95% CI + min/max。同一实例复用只记 `oracle`。
- 主矩阵 `workers = C` 个物理核，`taskset` 钉在这些核上，不超订。8 worker / 4 核只做 D 的对照。
- 本机只跑焦点块 15274915 与 3356896。不做 85 块扫描。
- `ge_1_5=false`，除非按本规格 R 的 95% CI 整体高于 1.5。不发明数字。

**完成标准：** `docs/specfence-soft0-execute-inflation-dig.md` 有 A–G 和 TPS 曲线表。`results/soft0-execute-inflation/` 有原始 JSON。脚本可在 ict21 上对 C=1..256 重跑。

## 步骤

1. **探针与入口** — 已完成。`SPECFENCE_INFLATION=1` 才记每笔相位、DAG、边界 F。`SPECFENCE_PIN_CPUS` 钉工人。`SPECFENCE_STEP_TRACE=1` 记访问偏移，关旗不进 `inspect_run`。
2. **同钟串行** — 已完成。Ideal 的每笔串行代价用 OCC workers=1 的 `ExecPhase.total_ns`。SEQ 的 `transact+commit` 只作为 basis A。
3. **本机 C=1/2/4** — 已完成。关旗墙 K=30（C=4 大块 28 轮，round 28 SF 活锁已记 `hang-c4.txt`）。8-on-4、READS、PERF 未跑，文档写明。
4. **步级偏移** — 已完成。`step-trace.jsonl` K=3。`Ideal_step` 在 `C1.json` / `C2.json` / `C4.json` / `curves.json`。WAW 后继用第一次写。
5. **seq!=par** — 已完成。产品 `Pevm::execute`、关旗、N=10。薄块 10/10 相等。大块 1/10 `seq!=par`（tx 91）。未改产品。
6. **文档** — 已写入 `docs/specfence-soft0-execute-inflation-dig.md`，含「Step-level offsets & Ideal_step」。`ge_1_5=false`。

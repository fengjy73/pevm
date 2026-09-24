# Soft0-InterpDeltaSplit 测量探针

**日期：** 2026-09-24（北京时间）
**起点：** `599e69a`（PR #54 tip，`cursor/soft0-occ-faster-same-evm-dig-7e0a`）
**性质：** 只加时钟。不砍 VmDb，不砍 Detect，不改 pick / 验证 / 发布。

## 目标

把 `run_pevm_tx` 拆成 opcode / 解释器核、VmDb 读、Detect-on-kept，在 Soft=0 Instant-off 上对 15274915（主）和 3356896 归因。回答差主要在 opcode（停刀）还是 VmDb/Detect（切路径仍 CONDITIONAL）。

## 约束

- 无产品切。无 Estimate、park-all、#47、HPC、QuietExit 再拧、FirstExecCut 式跳过。
- `ge_1_5=false`。探针墙不替换锁定带 7.0–7.2 / 5.3–5.9。
- `seq≡par`，`occ_picks=0`，`spine≤1`，`est=0`，`soft=0`。
- 时钟默认关（`SPECFENCE_INTERP_SPLIT=1` 才打 Instant）。比较例程打开它。

## 完成标准

- 每次 Soft=0 运行打印 opcode / vmdb / detect / other，OCC 同一条路径也打印。
- 大块 N=5、薄块 N=3，release，LTO off，`taskset -c 0-3`，请求 8 核。
- `docs/specfence-soft0-interp-delta-split-probe.md` 给出归因和停刀或 CONDITIONAL 判语。

## 步骤

1. **已完成：** `3421771`。VmDb 方法计时；kept spine / WaitOnce 另计 Detect。opcode 是残差。`other` 恒为 0。比较例程打印 `SPLIT`。
2. **已完成：** release、LTO off、`taskset -c 0-3`、8 核。大块 N=5 复用中位 SF 7.687 / OCC 5.795，ratio 0.754，`ge_1_5=false`。两块 `seq=par ok`，`occ_picks=0`，`spine_cores_max=1`。
3. **已完成：** `docs/specfence-soft0-interp-delta-split-probe.md`。四次复用的正差在 Detect-on-kept，不在 opcode。产品切仍 CONDITIONAL，本轮不切。

# Soft=0 busy / stall dig

**目标：** 在不改调度、Avoid、Admit、Learn 的前提下，把大块墙相对 Ideal 仍要收的约 3.1–3.5 ms 归到 (A) 超出 Ideal Σwork 的忙工作 和 (B) 有序脊 / refuse 停顿。

**约束：** 探针默认关（`SPECFENCE_BUSY_STALL=1` 才记）；关旗行为与现在相同。不发明墙。锁定带 SF 7.0–7.2 / OCC 5.3–5.9 不替换。Ideal：15274915 \(L_\mathrm{crit}\)=1.19，Σwork=3.02。

**完成标准：** `docs/specfence-soft0-busy-stall-dig.md` 有逐轮表、0.25 ms 时间箱、实际关键路径、排序桶；原始 JSON 在 `results/soft0-busy-stall-dig/`；PR 只带探针和文档。

## 步骤

1. **探针** — 已完成。`SPECFENCE_BUSY_STALL` 默认关。`ordered_pred` 在链头不再用 eager `then_some`。
2. **同机 SF 与 OCC** — 已完成。关旗大块复用中位 SF 7.539 / OCC 5.428。探针产品样轮 handoff 为 0；两轮 12.7 ms 是探针放大。`seq`：探针开与薄块关旗为 `seq=par`；关旗大块 N=5 打印过一次 `seq!=par`。
3. **脚本与文档** — 已完成。结论在 `docs/specfence-soft0-busy-stall-dig.md`。最大 SF 专有桶是首次执行超额约 1.5 ms 墙，类是壳税。有序停顿不是剩下的 3 ms。

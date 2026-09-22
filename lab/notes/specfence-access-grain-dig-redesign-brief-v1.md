# 访问粒度挖 + 重做（简报）

**日期:** 2026-09-22
**全文:** [`specfence-access-grain-dig-v1.md`](specfence-access-grain-dig-v1.md)、[`specfence-detect-avoid-resolve-learn-redesign-access-v1.md`](specfence-detect-avoid-resolve-learn-redesign-access-v1.md)
**JSON:** `lab/results/access-grain-dig/`
**不落地。** Soft=0。不恢复 `next_task*`。

四块（`19469097`、`19860366`、`15274915`、`3356896`）的跳数关键路径是一次很早的 **basic WAW**，不是交易粘性 Opt，也不是存储 RAW 星。`15274915` 的 ℓ 在 k=3（76/77 笔），`3356896` 的 ℓ 在 k=5/6（16/17 笔）。校验 `n_invalid=1`，rebind=0，FullReplay/OrderedReplay 仍从 k=0 重跑。

`19469097` tx 335 和 `19860366` tx 429 是 k=1 的 **beneficiary** 读。结构 DAG 没有这条边。复用把 `location_predicted` 打开后，429 对 coinbase 反复 Blocking。`19860366` tx 329 是真 WAW（peer 314，k=1），PE 已经是 1，仍 Blocking 数千次。

重做按这一次访问：beneficiary / `basic_lazy` 永远 Opt；真活前写只等一次并在发布后放行；Resolve 保留失败 k 之前的前缀，不为变值的账户做 rebind；Learn 只输出下一次 `(ℓ, k)` 的臂。交易粘性 Opt、E5→sticky Opt、以及不能改变下一次这个 ℓ 的信号退出。

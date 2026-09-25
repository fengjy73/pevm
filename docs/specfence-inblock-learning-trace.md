# 块内学习为什么压不住同一次新鲜 SpecFence 的重执行

**结论：** 块内 WaitOnce 有被读到，而且不是阈值太高、也不是键完全对不上。它压不住重执行，是因为武装只覆盖「此刻已经出现在多版本里的写者」。同一位置上还没出现的后续写者仍然让读者 FullReplay。真正把重执行打下来的是上一个块留下的有序写者链，只在下一次 `AccessSpine::begin` 装进 `ordered_writers`。新鲜块这条链是空的，块内不会补上。

## 怎么量

`SPECFENCE_INBLOCK_TRACE=1` 时才记。关旗时计数函数直接返回。产品路径不读这些计数。

每个配置先 `reset_heat` + `reset_inter_prior`，新的 `Pevm`（`spine_prior` 为空）跑一次 `fresh`，再用同一个 executor 跑一次 `carry`。脚本：`scripts/specfence_inblock_trace.sh`。下面的数字是 release、LTO off、`taskset -c 0-3` 的单次，不是中位数。

| 块 | 核 | 轮 | 墙 ms | reexec_entries | full_replay | protect_n | protect_before_opt | replay_after_protect | consult_no_pred | consult_opt | chain_len | ordered_handoff |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 15274915 | 4 | fresh | 29.200 | 122 | 50 | 1 | 171 | 49 | 2 | 0 | 0 | 0 |
| 15274915 | 4 | carry | 6.668 | 3 | 3 | 4 | 5 | 1 | 0 | 0 | 77 | 76 |
| 15274915 | 8 | fresh | 61.216 | 165 | 82 | 1 | 198 | 81 | 6 | 0 | 0 | 0 |
| 15274915 | 8 | carry | 8.032 | 27 | 14 | 12 | 54 | 20 | 14 | 0 | 77 | 76 |
| 3356896 | 4 | fresh | 2.869 | 20 | 20 | 1 | 34 | 19 | 0 | 1 | 0 | 0 |
| 3356896 | 4 | carry | 1.326 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 16 | 15 |
| 3356896 | 8 | fresh | 2.449 | 17 | 17 | 2 | 31 | 15 | 0 | 0 | 0 | 0 |
| 3356896 | 8 | carry | 1.802 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 16 | 15 |

15274915 C=4 新鲜轮的 incarnation：0 有 1202 笔，1 有 2，2 有 6，3 有 7，4 次及以上有 9。第一次武装在 tx 602、块起点后 0.89 ms。那之后才第一次开工、并且最终 incarnation>0 的有 20 笔；武装前开工且重执行的只有 4 笔。

## 五个候选

- **块内从未被咨询。** 不成立。新鲜轮 `protect_before_opt` 是 171 / 198 / 34 / 31，武装之后的读进了 `consult_ungated_wait_once`。`consult_opt` 几乎是 0，没有成批退回乐观读。
- **相对读者的读武装太晚。** 不是主因。大块新鲜轮里，大多数最终重执行的交易是在第一次 `protect_hot` 之后才开工的（C=4：20 对 4；C=8：58 对 2）。
- **粒度或键错了。** 不成立。`replay_after_protect` 几乎等于 `full_replay`（49/50、81/82、19/20、15/17）。FullReplay 就落在已经保护的那个位置上。
- **阈值太高。** 不成立。`protect_hot` 在第一次非 Commit 冲突就武装，新鲜轮 `protect_n` 是 1 或 2，不是 0。
- **武装了仍然 abort。** 成立，而且原因比「标志没生效」更具体：读者等到的是当前可见的写者。链上下一个还没写进多版本的交易不会被等。等完或看到一次发布之后，验证仍然 FullReplay。

## 块内状态实际做什么

- `AccessArmTable::protect_hot` 把该位置标成 WaitOnce，peer 保持 0。`consult_ungated_wait_once` 会读这个标志。
- `ordered_writers` 只在 `AccessSpine::begin` 从 `spine_prior.chains` 拷贝，块内没有写入口。新鲜轮 `chain_len=0`，`ordered_handoff=0`。
- 同一次 executor 的下一轮，`chain_len` 变成 77（15274915）或 16（3356896），`ordered_handoff` 是链长减一。重执行从 122 降到 3，或从 20 降到 1。薄块的 carry 轮 `protect_n=0` 且 `full_replay=0`，没有新的块内 WaitOnce，只靠这条链。
- `spine_prior` 的 Avoid 表在 begin 时是空的（`prior_radar_only=1`）。加速来自链本身被装进 `ordered_writers`，不是来自雷达权重。

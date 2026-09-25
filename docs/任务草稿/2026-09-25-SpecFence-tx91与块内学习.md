# SpecFence tx91 正确性与块内学习

**目标：** 修掉块 15274915 上产品 `Pevm::execute` SpecFence 的 `seq!=par`（第一笔分叉 tx 91）。然后诊断新鲜单次 SF 跑为什么块内反馈压不住重执行。不改 SpecFence 学习策略。

**约束：** 正确性只需几十次复现，不跑 200 次矩阵。停滞/活锁和 `spine_prior` 全量重置 API 降级，除非和根因是同一条路径。

**完成标准：**

- 说明 tx 91 哪一次读信了旧版本、哪条 SF 原语让它通过、为什么验证没拦住，并修根因。
- 几十次 C=4 产品路径上 15274915 不再 `seq!=par`。
- 块内学习用最小痕迹区分：从未在块内被咨询、武装太晚、粒度错、阈值太高、武装了仍 abort、或其他。结论写入 `docs/specfence-inblock-learning-trace.md`。

## 步骤

1. **tx91 根因** — 部分完成。`ST_RUNNING` 上 `wake_idle` 丢重验证是真洞，`reads_dirty` 已接上，单测已过。本机 N=24 无关探针全绿且 `commit_rejects=0`，没有再碰到 tx 91。人为拉开 Commit 窗口后，脏标志会响，但 tx 102 仍分叉且块末读原点有效；tx 106 读原点无效却能退出。不能把历史 tx 91 算成已被这条路径解释掉。
2. **复现确认** — 已完成。产品路径 C=4 块 15274915，N=24，diverge=0。窗口探针另记在经验记录，探针代码已撤。
3. **块内学习痕迹** — 已完成。结论：WaitOnce 被咨询，但仍 FullReplay；加速来自下一轮才装上的 `ordered_writers`。见 `docs/specfence-inblock-learning-trace.md`。

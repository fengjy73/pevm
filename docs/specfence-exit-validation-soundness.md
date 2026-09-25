# SpecFence 退出与读值校验

块可以在某一笔交易的读集已经对不上最终多版本内存时结束。读原点只记 `(tx_idx, incarnation)`，同一个 incarnation 换成另一份字节后，校验仍然通过。两处都修在验证和退出上，不改学习策略，也不改 Admit / Ideal-ready 的调度形状。

## 根因

**退出条件相信调度标志，不重读多版本内存。** 快路径在 `validated_tally_reached`、队列空、wave 空、handoff 空时直接 `break`。QuietExit 在「没有人跑、也没有欠验证」时同样离开。这些标志在 `Validated` 上，不会在写者已经发布之后再走一遍读集。

漏通知的一条具体路径是预验证提交。`finish_execution` 对 tx 0 和 lazy 交易立刻标成 `Validated`。随后 `apply` 的 Commit 进 `try_commit_if_clean`，状态不是 `Executed`，得到 `Closed` 并返回。`enqueue_higher_revalidate` 不跑。更高的读者若已经对着旧版本提交，就保持 `Validated`。`ST_RUNNING` 也不会在这条路径上 `mark_done`。工人随后从快路径离开，块结束时读集可以已经无效。

**原点相等不代表解释器看到的值相等。** `origin_still_valid` 只比对 `(tx_idx, incarnation)`，不比对 `MemoryValue`。`record` 按 `tx_idx` 覆盖 `MemoryEntry::Data`。同一 incarnation 的第二次 `record` 换掉金额，读者手里的原点身份不变。`invalidate_partial_suffix` 先把 Data 收成 Estimate，且不增加 incarnation、也不盖 aborted 戳；同一次 incarnation 再 `record` 时写入另一份 Data。读者若在第一份值上执行过，退出时身份仍然匹配。`occ_read_set_valid` 因此跳过重验证，Commit 可以粘住。

快进有同一形状。`try_ff_storage` / `try_ff_basic` 在 `last_data_before` 的 `(tx, inc)` 等于快照原点时直接返回快照字节，不读活值。跳转种子路径已经比过值；普通快进没有。WAR pin 把保留的旧值放进本地历史并记那个 incarnation。活表若后来是同一 incarnation 的另一份 Data，身份校验通过，解释器用的是 pin 上的旧字节。

tx 102 的收据差落在这条值上，而不是「退出时身份已经失败」。PR #64 的观察是：两边都 revert，累计 gas 5990773 对 6000599，差 9826，等于发送方余额差除以 25 gwei。块结束时 tx 102 的原点身份仍然匹配，所以单靠身份校验不会把它打回。tx 106 则是另一笔：身份已经无效，工人还是退出了。

## 不变量

块返回之前，每一笔交易的已记录读集都对着**最后一次写**之后的版本校验过。对多版本原点，校验的是解释器当时消费的 `MemoryValue`，不只是 `(tx_idx, incarnation)`。存储原点仍然表示这条链上没有更低的写者。

## 修复

**读原点带上值。** `ReadOrigin::MvMemory(TxVersion, MemoryValue)`。`origin_still_valid` 在身份之外要求活 `Data` 的值相等。`current_read_origins` 和每一次构造原点的读路径都写入当时的值。`prior_read_value_stable` 用原点上保存的值去比当前值，不再去读已经被原地覆盖的槽。同 incarnation 的 lazy 金额改写，以及 Estimate 被同一 incarnation 换成另一份 Storage，都会使 `validate_read_locations` 失败。

**快进不得交回一份身份相同、字节不同的快照。** 原点匹配但活值不同时 `try_ff_*` 返回 `None`，解释器走正常读，原点记下真正读到的值。

**预验证写者仍然通知更高读者。** `Closed` 且该 incarnation 已是 `Validated` 时调用 `enqueue_higher_revalidate`，并 `mark_done` 放下本核的 claim。Aborting / Ready 的 Closed 仍只释放 claim，避免拆掉另一个 incarnation 的 `ST_RUNNING`。

**最后一核退出前再验。** 队列空、没有人在跑、没有未完成或欠验证的交易时，按 `write_epoch` 序号锁扫描全部 `validate_read_locations`。序号在 `record` 和 Estimate 安装前后各加一（奇数表示写正在发布）。扫描中途序号变了就重扫。有失败的读集则 `mark_reads_dirty` 并 `wake_idle(Revalidate)`，本核不退出。别的核还在任务里时，空闲核可以先走；最后离开的核负责这次扫描。陈旧的 spine / sleep 位不再挡住「全员已验证」的这次扫描，否则快路径改完之后块会在计数已满时转死。

学习、AdmitShard、Ideal-ready 和 WaitOnce 的武装方式没有改。

## 证据

**加宽 Commit 窗口不再打出 PR #64 那次分叉。** 在修复前的探针上，块 15274915、C=4、`SPECFENCE_COMMIT_GAP_US=2000` 共 8 次，以及 `5000` 共 20 次：全部 `seq=par`，退出时 `invalid_n=0`，`rewrites=0`。脏标志会响（例如一次拒绝 25 笔）。PR #64 已经盖住「主人还在 `ST_RUNNING`、写者在 Commit 锁之前把读集标脏」。剩下的洞不在这段睡眠里。

**丢掉更高读者通知可以强制打出退出洞。** 修复前 `SPECFENCE_SKIP_FANOUT=1`，同一块 C=4：退出审计 `invalid_n=73`，工人仍然返回，`seq!=par`。第一处不等的收据是 tx 105（gas、状态、日志条数相同，结果结构仍不相等）。修复后同一钩子：`invalid_n=0`，`seq=par`。钩子已从树上删除。这就是「块在读集无效时结束」；tx 106 是同一种漏通知，不是另一套调度。

**值不等而身份相等有单测。** `same_incarnation_rewrite_fails_validation`：tx 0 以 incarnation 0 写入 `LazySender(100)`，tx 1 记下这个值，tx 0 再以同一 incarnation 写入 `LazySender(50)`，tx 1 的读集失败。`estimate_replaced_under_same_incarnation_fails_validation`：先写入 `Storage(3)`，`invalidate_partial_suffix` 收成 Estimate，再以同一 incarnation 写入 `Storage(8)`，读者失败。修复前 `origin_still_valid` 对这两种都会返回真。

**产品路径。** 探针删除之后，同一进程里顺序执行再并行执行，两边收据相等。每个格子 12 次，没有失败。

| 块 | C=4 | C=8 |
| --- | --- | --- |
| 15274915 | 12/12 `seq=par` | 12/12 `seq=par` |
| 3356896 | 12/12 `seq=par` | 12/12 `seq=par` |

单测 `same_incarnation_rewrite_fails_validation`、`estimate_replaced_under_same_incarnation_fails_validation`、`quiet_exit_on_block_quiet_and_not_while_work_remains` 通过。QuietExit 仍允许「有人在跑时，空闲核先离开」；欠验证且无人在跑时不能离开。这次没有再打出 gas 5990773 对 6000599，也不把历史的 tx 91 算进这条路径。

**仓库磁盘测试。** `rise_blocks_from_disk` 通过。`mainnet_blocks_from_disk` 在父提交 `a4298c4` 上就已经失败，这次没有把断言或哈希挪开。15274915 在顺序结果等于并行结果之后，收据根对不上块头：`0xb34ebba5…` 对 `0x30951acf…`，父提交是同一对。3356896 同样在顺序等于并行、收据根和 bloom 都通过之后，块头 gas 4033966 对执行累计 4014166，父提交是同一对。默认引擎是 OCC。把第一次恐慌接住再跑完时，4 个块整段通过，95 个块失败；失败原因没有逐块分类，上面两块的失败点与父提交相同。

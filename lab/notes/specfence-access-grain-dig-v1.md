# 访问粒度挖：`(tx, inc, k, ℓ, R|W)`

**日期:** 2026-09-22
**引擎:** [#45](https://github.com/fengjy73/pevm/pull/45) `db76e8b` 上的产品 Soft=0 路径。探针不入库。
**块:** `19469097`、`19860366`、`15274915`、`3356896`。一次冷启动、一次复用，请求 8 核。
**JSON:** `lab/results/access-grain-dig/<block>.json`
**相关 SoT（不是本挖的结论）:** [`specfence-complete-architecture-v4-finegrain.md`](specfence-complete-architecture-v4-finegrain.md)。那份把决策粒写成访问事件。下面的边是这四块上量出来的，不是那份草案的复述。

---

## 0. 结论

这四块的关键路径不是一笔交易的粘性 Opt，也不是一条存储 RAW 星。单位跳最长链上的冲突，几乎都是**同一个账户的 Basic 写后写**，而且在这笔交易很靠前的一次 `basic` 读上就已经知道：

| block | 单位跳 L | 链上笔数 | 共享 ℓ | 种类 | 最早 k | 复用 rebind |
|------:|--------:|--------:|:-------|:-----|-------:|------------:|
| 19469097 | 47 | 47 | 多条 Basic/Storage WAW | 见下 | 链上各笔不同 | 0 |
| 19860366 | 33 | 34 | 同上，外加两笔空转 | | | 0 |
| 15274915 | 77 | 77 | `abd6bb397881` 出现在 76/77 笔 | basic，不是 coinbase | **3** | 0 |
| 3356896 | 17 | 17 | `dff71d59d972` 出现在 16/17 笔 | basic，不是 coinbase | **5 或 6** | 0 |

`3356896` 上这条 ℓ 的校验计划是 FullReplay 19 次、OrderedReplay 19 次，`n_invalid=1`。值稳定 rebind 整块是 0。冲突在 k≈6 就成立，Resolve 仍把整笔打回去。

两笔空转是 **coinbase 的第一次 basic 读（k=1）**，不在结构 DAG 里（beneficiary 被排除）：

- `19469097` tx 335：ℓ = beneficiary，`basic`，k=1，peer 是刚写过 coinbase 的前一笔（331/334）。`pe=0`。没有 FullReplay。
- `19860366` tx 429：同样是 beneficiary，k=1。这一次复用 Blocking 3526 次，peer 427 写过该 ℓ，`pe` 几乎每次都是 1。Learn 把 coinbase 标成了预测位置，下一次访问还是 Blocking。

`19860366` tx 329 不是 coinbase。它是一条真的 Basic WAW：ℓ 的唯一前写是 tx 314，k=1，`gt=true`。复用 Blocking 7175 次，`pe` 7175。Learn 已经知道这个 ℓ，执行器仍然对同一次访问反复 Blocking，而不是等 314 发布之后读一次。

安静反链抽样（无前驱、`n_block=0`）没有门。它们的读是 Opt，这是对的。亏不在这里。

---

## 1. 方法

产品 Handler 路径在 PE 为空时不写 `AccessOrdinalLog`，`inspect_run` 也关着。本挖在 `VmDb::storage` / `basic` / `code_hash` 上加了只读探针：每次读记下 `(tx, inc, k, ℓ, 种类, 是否 Blocking, peer, 当时是否有未 done 的前写, location_predicted)`。写在执行结束、写集发布时记下，种类分开（storage / basic / lazy）。校验计划在 `validate_to_plan` 返回后记下，带第一次冲突的 ℓ、peer、类。探针关闭时不读多版本内存。没有改 Detect / Avoid / Resolve / Learn 的分支。

真边来自**同一次复用**的最终读写集，`dependency_edges(exclude_beneficiary=true, exclude_basic_lazy=true)`。RAW = 最近的前写者；WAW = 同一 ℓ 上相邻写者。WAR = 两个写者之间的读者。单位跳关键路径：`dist + to_sink - 1 = L` 的笔。这是跳数链，不是上一份按工作量加权的 13 笔路径。`15274915` 的工作量链只有十几笔；跳数链是 77 笔，其中 76 笔碰同一个 basic ℓ。

k 是这次 incarnation 里 `storage`/`basic`/`code_hash` 的序号。Block-STM 里别的交易要到写集发布才看得见这次写，所以「最早可知」按读者的第一次读，不按 SSTORE 操作码。写的 k 是发布顺序，不是操作码穿插顺序。

一次读算真边，仅当 `(peer, tx, ℓ)` 在 RAW 或 WAW 里。beneficiary 和 `basic_lazy` 被结构 DAG 丢掉，所以对它们的 Blocking 记成非 DAG 边，并单独标种类。

安静的 proceed 每笔只留前 32 条；Blocking、带活前写的 proceed、写、非 commit 的 Resolve 留到 4000。`n_read` / `n_block` / `n_pe` 是全量计数。`tx 329` 的事件有截断（`n_drop`），计数没有截断。

墙会晃。紧接着的两趟里，tx 335 的 Blocking 从约 3000 掉到 20，tx 429 从约 2200 到 3500，tx 329 从约 3800 到 7200。ℓ、种类、k、peer 是不是真写者，不跟着晃。下面的次数是后一趟复用。

Soft=0。

---

## 2. 块级

| block | n | RAW | WAW | WAR | 读 | Blocking | 其中无活前写 | proceed 时有活前写 | pe 读 | Full | Ordered | rebind |
|------:|--:|----:|----:|----:|---:|---------:|---------------:|-------------------:|------:|-----:|--------:|-------:|
| 19469097 | 336 | 28 | 182 | 147 | 14157 | 618 | 110 | 43 | 1433 | 144 | 85 | 0 |
| 19860366 | 430 | 34 | 236 | 143 | 39017 | 11275 | 3632 | 38 | 12405 | 153 | 71 | 0 |
| 15274915 | 1226 | 35 | 120 | 107 | 6474 | 43 | 15 | 70 | 1665 | 135 | 140 | 0 |
| 3356896 | 176 | 0 | 22 | 18 | 935 | 4 | 2 | 9 | 55 | 19 | 19 | 0 |

存储 RAW 只有几十条。WAW 是大头。`3356896` 的 RAW 是 0：这条链上没有存储读依赖。

聚焦集合里的 Blocking 按 ℓ 种类（后一趟）：

| block | storage | basic（非 coinbase） | beneficiary | basic_lazy |
|------:|--------:|---------------------:|-------------:|-----------:|
| 19469097 | 80 | 106 | 20 | 0 |
| 19860366 | 155 | 4194 | 3527 | 0 |

`19860366` 的门几乎全是账户，其中一半是 coinbase。

---

## 3. 真边和最早的 k

`15274915` 跳数链 77 笔里 76 笔读 `abd6bb397881`。种类 basic，不是 beneficiary。第一次读的 k 是 **3**。写者是更早的链上笔（116、122、…）。边是 WAW，不是 RAW。校验时 `n_invalid=1`，类是 EffectiveWAW。计划在 k=6 记下（这次 incarnation 又读了几下之后），FullReplay 或 OrderedReplay。rebind 是 0。

`3356896` 跳数链 17 笔里 16 笔读 `dff71d59d972`。种类 basic，不是 beneficiary。第一次读 k=5 或 6。同一 ℓ 上的计划：Full 19、Ordered 19，rebind 0。整块 Blocking 只有 4 次。这条链**不是**在访问时等前写，而是 Opt 读完，校验失败，整笔重放。前缀（k 小于这次 basic 读）被丢掉。

`19469097` / `19860366` 的链上笔同样是「一个 ℓ、k 很早、EffectiveWAW、`n_invalid=1`」。例子：`19469097` tx 83，storage ℓ `4fe4c64172b1`，k=5，peer 82 是真 WAW 写者，inc 0 的 vis=Opt 然后 FullReplay，inc 1 仍 FullReplay，inc 2 才 OrderedReplay。三次都是同一个 ℓ。

WAR 有 18–147 条（夹在两个写者之间的读者）。Resolve 记下来的第一次冲突类是 EffectiveWAW 或 commute，不是单独的 WAR 臂。

---

## 4. Detect 滞后，和没有真边的门

真 WAW 上，第一次 Blocking 就在第一次读的 k（3、5、6、10）。Detect 并不晚到校验才知道 ℓ。晚的是动作：要么当时不挡（`3356896` 整块只有 4 次 Blocking），把失败留到校验；要么挡了以后不撤，同一 `(tx, ℓ, peer)` 再挡几千次。

没有结构 DAG 边的门：

| tx | 块 | ℓ | 种类 | k | 次数（这一趟） | peer | peer 写过 ℓ | pe |
|---:|:---|:--|:-----|--:|---------------:|-----:|:-------------|---:|
| 335 | 19469097 | beneficiary | basic | 1 | 20 | 331、334 | 是 | 0 |
| 429 | 19860366 | beneficiary | basic | 1 | 3526 | 427 为主 | 是 | 1 |
| 9 | 3356896 | `758664a53cc5` | basic_lazy | 1 | 1 | 8 | 是 | 0 |
| 9 | 15274915 | `939b847e3af4` | basic_lazy | 1 | 1 | 8 | 是 | 1 |

335 和 429 的「假边」是：结构 DAG 按约定丢掉了 beneficiary，产品路径仍把 coinbase 的第一次 `basic` 读当成要等的前写。peer 确实写过这个账户（每笔交易都碰 coinbase），所以这不是随机 peer，是**不该拿来排序的 ℓ**。冷启动时 429 只有 3 次 Blocking；复用把 `location_predicted` 打开之后变成几千次。Learn 把 coinbase 学成了要点，空转是学出来的。

tx 9 是被丢掉的 lazy 余额边，各 1 次，不是空转。

`19860366` 有 3632 次 Blocking 发生时，这次读看不到未 done 的前写（`n_block_no_live`）。门比前写的生命周期长。

---

## 5. 同一次访问上的 Avoid

探针按这次读记下 vis（0 Opt，1 WaitReleased，2 OrderedTip）和有没有活前写。

- 安静反链：`n_block=0`，`n_proceed_live=0`。例如 `19469097` tx 0 / 87 / 191 / 267，`19860366` tx 129 / 249 / 352，`15274915` tx 0 / 268 / 625 / 989，`3356896` tx 0 / 37 / 87 / 143。这些读就是 Opt。没有多挡。
- 真 WAW 的第一下经常仍是 Opt。`3356896` tx 67：k=6 的 basic 读，`proceed_while_live=1`，然后 inc 0 是 OrderedReplay（vis 0），inc 1 是 FullReplay（vis 1），inc 2 又是 OrderedReplay（vis 2）。同一次 ℓ 读，三种 vis，没有一次停在「读到已发布的值然后提交」。
- `15274915`：1665 次 pe 读，Blocking 只有 43。PE 打开了，这次访问仍走 Opt，失败留到 FullReplay / OrderedReplay（135 / 140）。
- 活前写还在时仍 proceed 的次数很小（43 / 38 / 70 / 9）。大头不是「活前写还在却 Opt」，而是「前写已经不是 done 意义上的活，或者根本是 coinbase，却 Blocking / 整笔重放」。

Avoid 的单位仍是这笔交易的 vis，不是这次 `basic`/`storage`。同一 incarnation 里 k=1 的 coinbase 和 k=6 的真 WAW 共用一个 vis。

---

## 6. 失败的那一次访问上的 Resolve

校验给出的第一次冲突是一个 ℓ，`n_invalid=1`，类 EffectiveWAW（`3356896` tx 31 有一次 commute）。rebind 四块都是 0，rewind 这一趟也是 0。

值变了的账户 WAW 不能做值稳定 rebind，这和 rebind=0 一致。能救的是这次读之前的前缀：`3356896` 上冲突读在 k=5 或 6，`15274915` 在 k=3。FullReplay 和 OrderedReplay 都让下一 incarnation 从 k=0 再跑到同一个 k。OrderedReplay 换了 vis，没有跳过已完成的前缀。

tx 329 的 Resolve 计数是 0 次 Full、0 次 Ordered。它没有走到「重放一次」，而是在 k=1 的那次 basic 读上 Blocking 七千次。incarnation 不涨，前缀无从谈起。

---

## 7. Learn 有没有改变下一次对同一个 ℓ 的访问

聚焦集合里，冲突之后的下一 incarnation 仍以 Opt（vis=0）再读这个 ℓ 的次数：50 / 24 / 33 / 12。下一 incarnation 上 `location_predicted=1` 的次数更高：84 / 74 / 247 / 38。

PE 位变了。这次访问的结局没有变成「只重读这个 ℓ / 保留前缀」。结局仍是 FullReplay、OrderedReplay，或（429、329）再 Blocking。

tx 429：冷启动 pe=1、Blocking=3；复用 pe 与 Blocking 同量级（约 3500）。学到的是「这个 ℓ 要预测」，ℓ 是 coinbase。下一次 k=1 的 basic 读更频繁地停住。

tx 329：pe 与 Blocking 同量级，ℓ 是真 WAW，peer 314。预测没有把重复 Blocking 收成一次等待。

没有一条 Learn 输出是 `(ℓ, k, 形态)` 上的「下一次这个访问用哪个臂」。E5、sticky Opt、tx 级策略名都不在这次读的记录里，它们也没有改掉上面的 k。

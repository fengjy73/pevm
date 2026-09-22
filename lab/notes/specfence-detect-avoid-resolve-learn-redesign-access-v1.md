# Detect → Avoid → Resolve → Learn，按访问和边重做

**日期:** 2026-09-22
**依据:** [`specfence-access-grain-dig-v1.md`](specfence-access-grain-dig-v1.md)，JSON `lab/results/access-grain-dig/`。
**相关 SoT:** [`specfence-complete-architecture-v4-finegrain.md`](specfence-complete-architecture-v4-finegrain.md) 把决策粒写成访问事件 \(a=(t,\mathrm{inc},k,\ell,\mathrm{mode})\)，Learn 写成 \((\ell,k,\mathrm{morph})\)。那份是草案，而且自标已被冻结粒度文档取代。本文件只用这次四块挖到的边，不按那份草案落地。
**性质:** 只设计。不改代码。没有 P0/P1/P2。Soft=0。不恢复 `next_task*`。

---

## 0. 这次访问长什么样

四块上该管的对象是一次读，不是一笔交易：

- `15274915` 跳数链 76/77 笔、`3356896` 16/17 笔，碰的是**同一个非 coinbase 的 basic ℓ**，第一次读在 k=3 或 k=5/6。校验 `n_invalid=1`，类 EffectiveWAW。rebind=0。FullReplay 和 OrderedReplay 把整笔从头再跑。
- `19860366` tx 329：真 Basic WAW，k=1，peer 是唯一前写 314。`location_predicted` 已经是 1，同一次读仍 Blocking 数千次。
- `19469097` tx 335、`19860366` tx 429：k=1 的 **beneficiary basic**。结构 DAG 没有这条边。复用把预测位打开之后，429 的下一次 k=1 读变成数千次 Blocking。
- 安静反链的读是 Opt，没有门。存储 RAW 只有几十条，不是这些空转和这条链的主体。

所以协议只回答四个问题，而且都钉在 `(tx, inc, k, ℓ, R|W)` 上。

---

## 1. Detect

每次 `basic` / `storage` / `code_hash` 都记下这个 k 和 ℓ。这是记录，不是门。

最早可知的时刻就是这次读的 k：链上是 3 或 5/6，coinbase 空转是 1。不需要等到校验才知道 ℓ。`3356896` 整块只有 4 次 Blocking，说明产品路径经常记了（或能记）却在这次读上不挡，把 EffectiveWAW 留到整笔重放。

不记成边的 ℓ：

- beneficiary。335 和 429 的 peer 确实写过 coinbase，那仍然不是 RAW/WAW 调度边。
- `basic_lazy`。tx 9 对 peer 8 的那一次就是这种，各块 1 次。

存储 RAW 和相邻 Basic/Storage WAW 才是边。边的身份是 `(前写 tx, 这次 tx, ℓ, 读者的 k)`，不是 `(ℓ, reader)` 抹掉 k，也不是「这笔交易在等」。

---

## 2. Avoid（只对这一次访问）

```
on read a = (tx, inc, k, ℓ, R|W):
  if ℓ is beneficiary or basic_lazy:
    Opt this read                          # OCC cost, no edge
  else if a live unfinished RAW/WAW writer w of ℓ exists:
    wait once for w to publish ℓ
    then read that published value
  else:
    Opt this read
```

等待的范围是这一次读。k 更大的另一次读另做决定。k=1 的 coinbase 不得把 k=6 的真 WAW 一起标成同一笔 vis。

「等一次」是对 tx 329 的直接约束：真前写是 314，k=1，PE 已经是 1，产品却 Blocking 了几千次，incarnation 不涨。同一 `(tx, ℓ, w)` 在 w 发布之前只停一回；w 发布或 done 之后这次读必须往下走，不许再入队。

「无活前写就 Opt」是对 `3356896` 链的另一半：那里 Blocking 几乎没有，失败发生在校验。Opt 只在这次读的前写已经发布、版本对得上时成立。对不上就落到第 3 节，而不是默默读完再整笔重放。这一趟里「proceed 时仍有活前写」只有 9–70 次，不是主税；主税是真 WAW 没在这次 k 上结束。

安静反链保持现在这样：不挡。

---

## 3. Resolve（救这一次读和它前面的前缀）

这四块上 rebind=0。账户 WAW 的值是变的（nonce / 余额），值稳定 rebind 不该被当成主路径，也不该再调参去逼它发生。

失败记录已经是一个 ℓ、`n_invalid=1`、k 在 3–6。Resolve 只处理这次读：

- 前写已发布：用发布的值完成这次读，保留 k 小于失败点的前缀，从失败点继续。不从 k=0 FullReplay。
- 前写未发布：就是第 2 节的等一次。等完再读。不 OrderedReplay 整笔。
- 前缀保不住（失败点之前的读依赖这个 ℓ，或身份丢了）：才整笔重放。这是漏预测之后的 OCC 代价，不是默认计划。

现在的 OrderedReplay 只把 vis 从 0 改成 1 或 2，下一 incarnation 仍从 k=0 跑到同一个 k（`3356896` 上计划记在 k=9，第一次冲突读在 k=5/6）。那不是前缀跳过。

tx 329 走不到 Resolve。它在 k=1 被重复 Blocking。先停掉重复门，才谈得上救前缀。

---

## 4. Learn（只学能改变下一次这个 ℓ 的东西）

合法特征是 `(ℓ, k, 类)`：类是 storage-RAW、basic-WAW、storage-WAW。beneficiary 和 `basic_lazy` 不进这个键。

合法输出只有一个：下一次碰到同一个 `(ℓ, k)` 时，这次读用 Opt，还是等那个还活着的前写。输出必须在下一次读之前装上。装不上的信号删掉，包括：

- 交易粘性 Opt / Wait。聚焦集合里，冲突后的下一 incarnation 仍以 vis=0 再读同一 ℓ（50 / 24 / 33 / 12 次）。策略名没有落到这次访问上。
- E5 → sticky Opt。它不改变 k=3 或 k=6 的下一次读。
- 只把 `location_predicted` 设成 1。tx 429 的 coinbase、tx 329 的真 WAW，pe 和 Blocking 同量级。预测位没有把下一次访问收成「等一次」或「不要等 coinbase」。
- E1 计数、E4、E6、探索预算、bandit。它们不选择这次读的臂。

学到 basic-WAW `(ℓ, k≈3..6)`：下一 incarnation 在这个 k 等前写发布，而不是 Opt 完整跑完再 FullReplay。

学到 beneficiary：下一 incarnation 的 k=1 **继续 Opt**。429 的复用是反例，那个 Learn 不该存在。

默认：不是这条边的访问，代价就是 OCC 的这一次读。存储上的冷 ℓ、反链上的读，保持现在的 Opt。

---

## 5. 和 v4.1 细粒度草案对齐的地方、以及这次证据改写的地方

对齐：决策不是「交易 t 在等」；Detect 不等于 Fence；Resolve 优先处理这次读和已认证前缀；Learn 的键带 k，不带交易粘性位。

这次证据改写的：

- 要点不是深 k 的 SLOAD 星。这四块的链是**很早的一次 basic WAW**（k=3 或 5/6），存储 RAW 是少数。
- 值稳定 rebind 在这些边上是 0。账户值变了。草案里的 RebindThis 不是这些块的主 Resolve。
- coinbase 必须显式排除。草案的「非要点 → OCC」覆盖它，但产品 Learn 已经把 beneficiary 学成 pe=1，并在 k=1 空转。
- 「等」如果实现成可重复的 Blocking，就会变成 tx 329。等一次，发布后必须放行。

不落地。下一轮若改代码，完成线是：这四块上 beneficiary 的 k=1 读不再 Blocking；tx 329 对 ℓ 的等待次数是 1 而不是数千；`3356896` / `15274915` 链上那次 basic 读失败后，下一跳从失败 k 继续而不是从 0 FullReplay；rebind 仍允许为 0；Soft=0；`occ_picks=0`；`seq=par`。

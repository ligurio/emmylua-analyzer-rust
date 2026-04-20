# Summary Builder 单文件语义统一推进文档

## 文档目标

这份文档替代以下三份已经发生分叉的旧文档：

1. `summary_builder_semantic_solver_architecture_CN.md`
2. `summary_builder_semantic_solver_checkpoint_CN.md`
3. `type_system_salsa_architecture_CN.md`

它只保留一条当前有效的路线，回答四个问题：

1. 当前仓库里已经稳定存在什么。
2. 当前真正的主矛盾是什么。
3. 接下来应该按什么顺序推进。
4. 每一阶段的退出条件和回归基线是什么。

## 一句话结论

当前项目已经不是“缺 graph / 缺 SCC / 缺 solver 宿主”的阶段。

当前真正的问题是：

1. 单文件语义已经有两条都很强的主线：`type_system` 的 program-point query 和 `semantic_solver` 的 solver-owned summary。
2. 这两条线都已经能产出稳定结果，但还没有完全收敛成同一套“权威语义层”。
3. 高层 consumer 仍大量停留在旧 `DbIndex + analyzer` 语义上。

因此当前最优先的方向不是继续扩 facade，也不是立刻做跨文件 graph host，而是：

1. 先把单文件权威语义层收口。
2. 再把高层 consumer 分批迁到这条新主路径。
3. 最后再进入跨文件宿主。

## 当前状态

### 1. 已经稳定存在的基础设施

当前仓库已经稳定具备：

1. syntax-first 的单文件 summary / facts。
2. `type_system` 下的声明候选、member 候选、program-point 候选与 narrowing。
3. semantic graph、SCC、worklist、component result 和最小 fixedpoint solver。
4. solver-owned 的 summary-first 公开读面：`signature / decl / member / for-range / module export / resolved doc type`。

这意味着“继续搭骨架”已经不是有效目标。

### 2. 当前单文件语义的真实分层

当前更准确的分层是：

1. `analysis/*`：稳定 syntax-first facts。
2. `query/type_system/*`：program-point 类型与局部语义解释。
3. `query/signature.rs`：call explain、signature return、overload return rows。
4. `query/semantic_graph.rs`：依赖图宿主。
5. `query/semantic_solver.rs`：component 级传播、fixedpoint 和 solver-owned summary。
6. `semantic/mod.rs`：对上层暴露新的 summary-first 读取口。

这里最重要的判断是：

1. `type_system` 不是前置设施，而是正在形成中的单文件权威 query 层。
2. `semantic_solver` 也不是试验性骨架，而是已经承担 component 聚合与对外 summary 的正式宿主。
3. 接下来必须决定两者如何协作，而不是继续各自扩面。

## 当前主矛盾

### 1. 不是“有没有 solver”

solver 侧已经具备：

1. component 调度。
2. predecessor 输入消费。
3. propagated / local / fixedpoint 分层。
4. decl/member/signature return/for-range/module export 的 summary-first 读面。

所以当前主矛盾不是“继续补一个 solver 入口”。

### 2. 不是“有没有 program-point query”

`type_system/program_point.rs` 已经覆盖：

1. local assignment 跟踪。
2. 基础 flow narrowing。
3. correlated overload row 过滤。
4. table shape 驱动的 member/index 行为。
5. 多返回调用在 decl assignment 上的 slot 精度。

所以当前主矛盾也不是“从零开始做 program-point query”。

### 3. 当前真正的主矛盾

当前真正要解决的是三件事：

1. 让 member/property 相关链路拥有和 decl 一样的多返回 slot 精度。
2. 让 solver 的 propagated/local/fixedpoint 不再只是字段分层，而是规则分层。
3. 让高层 consumer 默认读取 summary-first 单文件语义，而不是继续把旧 analyzer 当主实现。

## 统一推进顺序

## 阶段 1：收口单文件权威语义层

这是当前最高优先级阶段。

目标：

1. 让 `type_system` 的 program-point query 成为单文件局部类型真相源。
2. 让 `semantic_solver` 成为 component 聚合和公开 summary 真相源。
3. 明确两者的边界，不再重复扩相同能力。

### 阶段 1A：补齐 member/property 的多返回 slot 精度

当前已完成：

1. decl 声明式 initializer 已经保留 `value_result_index + source_call_syntax_id`。
2. solver 和 program-point 在 `local a, b = pair()` 上已经能按 slot 消费 call returns。

当前缺口：

1. member candidate 仍未统一携带 slot 元数据。
2. property candidate 仍未统一携带 slot 元数据。
3. 相关 alias / forwarded member 场景仍可能默认退回第 0 返回槽。

本阶段应先完成：

1. 把 slot 元数据提升到统一 candidate 层。
2. 让 member initializer 的 call path 按 slot 消费 call explain / signature return。
3. 补齐对应 semantic_solver 和 program-point 回归。

### 阶段 1B：收紧复杂 owner 下的 member/index program-point 规则

目标：

1. 收紧 alias、union、mapped owner、named type bridge 下的 owner -> member 候选桥接。
2. 明确保守回退边界，而不是继续依赖临时兜底。
3. 让复杂 owner 的 member/index 结果保持可解释、可测试。

这一步仍然以 query 为中心，不先动 consumer。

### 阶段 1C：固定“query 与 solver”的边界

统一约定：

1. 程序点局部类型结论优先来自 `type_system/program_point`。
2. component 聚合、cycle、predecessor 传播和公开 semantic summary 由 `semantic_solver` 负责。
3. 任何新单文件语义能力，先判断它是“局部程序点解释”还是“component 聚合传播”，不要双线重复实现。

阶段 1 的退出条件：

1. decl/member 的多返回 slot 精度对齐。
2. 复杂 owner 的 member/index program-point 行为边界固定。
3. 新增单文件语义需求能明确落到 query 或 solver 其中一边。

## 阶段 2：收紧 solver transfer 语义

这是当前第二优先级阶段。

目标：

1. 把 `propagated / local / fixedpoint` 从“结构分层”推进到“规则分层”。
2. 让 cycle component 的迭代依据不再只是通用 shell merge。
3. 为后续高层 consumer 迁移提供更稳定的 solver-owned summary。

应优先做：

1. 区分 doc-type、named-type、initializer-derived、call-return-derived 等证据来源。
2. 明确哪些证据可以传播、哪些只能本地消费。
3. 收紧 cycle transfer，而不是继续增加新的 façade。

阶段 2 的退出条件：

1. `propagated_value_shell` 与 `local_value_shell` 在行为上真正不同。
2. cycle 组件的 fixedpoint 不再主要依赖粗粒度 state/candidate union。
3. solver summary 的字段能够直接解释传播来源，而不是只暴露结果壳。

## 阶段 3：迁移高层 consumer

这是当前第三优先级阶段。

目标：

1. 让 `semantic/mod.rs` 暴露的 summary-first 入口成为默认高层读取路径。
2. 逐步削弱旧 `DbIndex + analyzer` 在高层语义中的主实现地位。
3. 保留 fallback，但让 fallback 真正退回兼容层。

建议顺序：

1. 先迁只读解释型 consumer。
2. 再迁 closure/signature 周边 consumer。
3. 最后再碰 `infer_expr_semantic_decl`、`type_check`、`diagnostic checker` 主路径。

理由：

1. 当前 `semantic/mod.rs` 已经提供 `signature_summary / decl_summary / member_summary / call_explain` 等入口。
2. 这层已经可以作为 consumer 切换桥接点。
3. 直接碰旧 infer/type_check/diagnostic 风险更高，应该后移。

阶段 3 的退出条件：

1. 高层只读语义查询优先经由 summary-first 入口。
2. 旧 infer/type_check/diagnostic 不再承担“单文件主语义解释器”的角色。
3. fallback 位置清晰、可枚举。

## 阶段 4：准备跨文件宿主

这是当前明确后置的阶段。

只有在阶段 1 到 3 足够稳定之后，才进入：

1. require/module export 跨文件桥接。
2. compilation 级 graph host。
3. dirty-region aware 的局部重算边界。

原因：

1. 如果单文件权威语义层还没有收口，跨文件宿主只会把分叉放大。
2. 如果高层 consumer 还没切主路径，跨文件 graph 也不会真正被消费。

## 当前不该优先做的事

当前不应优先投入：

1. 再扩一批 facade 名称或 query 名称。
2. 提前设计 compilation 级全图替换。
3. 直接删掉旧 analyzer。
4. 在 query 和 solver 两边同时实现同一条新语义规则。

## 回归基线

每次继续当前路线前，至少应跑：

1. `cargo test -p emmylua_code_analysis summary_builder -- --nocapture`
2. `cargo test -p emmylua_code_analysis semantic_solver -- --nocapture`
3. `cargo test -p emmylua_code_analysis signature_return -- --nocapture`

当阶段 1A 继续推进时，再额外锁定：

1. decl 多返回 slot 回归。
2. 新增 member 多返回 slot 回归。
3. 相关 program-point member 回归。

## 当前一句话行动建议

下一步直接做：

1. member/property candidate 的 slot 元数据统一携带。
2. member initializer 的 call slot 精度补齐。
3. 对应 semantic_solver + program-point 回归补上。

这一步做完之后，再进入 solver transfer 分层，而不是先改跨文件。
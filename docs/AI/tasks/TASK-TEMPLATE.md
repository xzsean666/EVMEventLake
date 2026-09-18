# TASK-xxx: [任务简明标题]

## Objective
[用 1~2 句话清晰描述该任务的单一目标和预期产出，说明为什么要做]

## Scope
[明确任务涉及的具体范围边界，列出包含哪些改动、不包含哪些改动]
- 包含：
- 不包含：

## Allowed Files
[严格限制该任务允许创建或修改的文件路径，原则上不超过 5 个实现文件和 3 个测试文件]
- `src/...`
- `tests/...`
- `docs/...`

## Dependencies
[列出该任务依赖的前置 Task 编号或外部前置条件。若无依赖则填写 None]
- 前置任务：None
- 外部依赖：None

## Inputs and Outputs
- **Inputs**: [任务所需的输入参数、数据结构、接口调用或配置项]
- **Outputs**: [任务产生的输出、修改的数据模型、接口返回值或工件]

## Acceptance Criteria
[列出具体的、可直接验证的验收标准条目]
- [ ] 标准 1：...
- [ ] 标准 2：...
- [ ] 标准 3：...

## Verification Commands
[列出可以实际执行以验证任务是否完成的命令]
```bash
# 示例：运行最窄相关测试
cargo test --test xxx

# 示例：静态检查与格式检查
cargo check
cargo fmt --check
```

## Risks and Assumptions
- 风险：[潜在破坏兼容性、性能下降或并发竞态风险]
- 假设：[关于依赖环境、配置或外部接口行为的假设]

## Status
TODO | IN_PROGRESS | REVIEW | DONE | BLOCKED

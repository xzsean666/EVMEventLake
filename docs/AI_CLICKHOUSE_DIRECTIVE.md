# 面向 AI Agent 的 ClickHouse 容器化调优与工程审查指令规范
(AI Agent Directive: ClickHouse Docker Optimization & Setup Guide)

> **如何给其他 AI 使用 (Usage Prompt)**：
> 当你在任何新项目或现有项目中需要 AI 代理（如 Cursor, Claude, Antigravity, Copilot, ChatGPT 等）为你配置、检查或优化 ClickHouse 时，直接将本文件提供给 AI，并附带以下 Prompt：
> 
> ```text
> 请作为资深大数据与系统架构师，严格阅读并遵循 docs/AI_CLICKHOUSE_DIRECTIVE.md 规范。
> 1. 全面审查本项目中所有涉及 ClickHouse 的 Docker/Compose 配置、服务端配置、客户端写入代码和 SQL DDL。
> 2. 检查是否存在“系统操作日志撑爆磁盘”和“高频写入产生碎片导致 Too many parts 卡死”两大隐患。
> 3. 直接帮我完成缺失配置的补充、代码参数的注入与表级设置调优，并输出诊断与修改报告。
> ```

---

## 1. AI 角色设定与第一守则 (Role & Core Objectives)

你是一个资深的大数据基础设施架构师。在任何使用 **Docker 容器化运行 ClickHouse** 的工程项目中，你必须严格防御以下两大生产级“致命痛点”：

1. **系统操作日志爆炸（System Logs Explosion）**：
   ClickHouse 默认全量记录 `query_log`、`part_log`、`trace_log` 且保留 30 天。在数据持续写入场景下，系统表体积会以惊人速度膨胀，数天内即可达到数十甚至数百 GB，远超实际业务数据本身，最终撑爆磁盘。
2. **碎片生成速度快于 Merge 速度导致服务卡死（Too Many Parts / 写入反压）**：
   ClickHouse MergeTree 每次 INSERT 生成一个独立磁盘 Part。如果客户端采用微批（Micro-batching）高频直写，Part 累积速度会远远超过后台 Merge 线程合并速度。一旦达到 `parts_to_delay_insert`（默认 150），ClickHouse 会强制休眠延迟写入拖死应用；达到 `parts_to_throw_insert`（默认 300），直接抛出 Code 252 异常拒绝写入，导致服务卡死或崩溃重试。

---

## 2. AI 标准审查与自动配置步骤 (5 步标准执行流)

### Step 1: 检查并配置 Docker / Docker Compose

当发现项目中有 ClickHouse 的 Docker 启动配置（如 `docker-compose.yml` 或启动脚本）时，AI 必须检查并确保具备以下 4 项核心配置：

1. **必须配置 `ulimits.nofile`**：
   Docker 容器默认文件句柄通常只有 1024，ClickHouse 极易报 `Too many open files` 崩溃。
   ```yaml
   ulimits:
     nofile:
       soft: 262144
       hard: 262144
   ```
2. **必须挂载自定义配置目录**：
   严禁让用户通过 `docker exec` 进入容器修改配置文件。必须将宿主机目录以只读方式挂载到官方容器：
   ```yaml
   volumes:
     - ./data/clickhouse:/var/lib/clickhouse
     - ./logs/clickhouse:/var/log/clickhouse-server
     - ./clickhouse/config.d:/etc/clickhouse-server/config.d:ro
     - ./clickhouse/users.d:/etc/clickhouse-server/users.d:ro
   ```
3. **必须限制 Docker 容器日志大小**：
   防止标准输出的 json-log 撑爆 Docker Overlay2 宿主机根分区：
   ```yaml
   logging:
     driver: "json-file"
     options:
       max-size: "50m"
       max-file: "3"
   ```
4. **必须配置健康检查（Healthcheck）**：
   ```yaml
   healthcheck:
     test: ["CMD-SHELL", "wget -qO- http://127.0.0.1:8123/ping | grep -q Ok"]
     interval: 5s
     timeout: 5s
     retries: 20
   ```

---

### Step 2: 检查并生成服务端 XML 配置文件

AI 必须确保项目中的 `clickhouse/config.d/` 和 `clickhouse/users.d/` 存在以下两个优化文件（若缺失必须立即创建）：

#### 1. `clickhouse/config.d/system_logs.xml` (系统日志截断与防爆)
```xml
<clickhouse>
    <!-- 0. 文件日志极致收敛：提升为 warning 级别（彻底忽略 trace/debug/information），单文件限制 20M，仅保留 1 份轮转 -->
    <logger>
        <level>warning</level>
        <log>/var/log/clickhouse-server/clickhouse-server.log</log>
        <errorlog>/var/log/clickhouse-server/clickhouse-server.err.log</errorlog>
        <size>20M</size>
        <count>1</count>
    </logger>

    <!-- 1. 收敛 query_log：保留期缩短至 1 天，调大刷新周期以降低磁盘 IO -->
    <query_log>
        <database>system</database>
        <table>query_log</table>
        <partition_by>toYYYYMM(event_date)</partition_by>
        <flush_interval_milliseconds>7500</flush_interval_milliseconds>
        <max_size_rows>1048576</max_size_rows>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </query_log>

    <!-- 2. 彻底移除采样追踪 trace_log 与分析日志 processors_profile_log，极大节约磁盘空间 -->
    <trace_log remove="1"/>
    <processors_profile_log remove="1"/>
    <opentelemetry_span_log remove="1"/>

    <!-- 3. 收敛 part_log（数据分片与合并审计）：保留 1 天自动淘汰 -->
    <part_log>
        <database>system</database>
        <table>part_log</table>
        <partition_by>toYYYYMM(event_date)</partition_by>
        <flush_interval_milliseconds>7500</flush_interval_milliseconds>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </part_log>

    <!-- 4. text_log：仅保留 warning 及以上日志，统一 1 天淘汰 -->
    <text_log>
        <level>warning</level>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </text_log>

    <!-- 5. metric_log / asynchronous_metric_log / asynchronous_insert_log：统一 1 天淘汰 -->
    <metric_log><ttl>event_date + INTERVAL 1 DAY DELETE</ttl></metric_log>
    <asynchronous_metric_log><ttl>event_date + INTERVAL 1 DAY DELETE</ttl></asynchronous_metric_log>
    <asynchronous_insert_log><ttl>event_date + INTERVAL 1 DAY DELETE</ttl></asynchronous_insert_log>

    <!-- 6. 提升后台合并（MergeTree Merge）线程池并发，加快小分片合并 -->
    <background_pool_size>16</background_pool_size>

    <!-- 7. 全局 MergeTree 写入防堵参数 -->
    <merge_tree>
        <parts_to_delay_insert>300</parts_to_delay_insert>
        <parts_to_throw_insert>600</parts_to_throw_insert>
        <max_delay_to_insert>1</max_delay_to_insert>
    </merge_tree>
</clickhouse>
```

#### 2. `clickhouse/users.d/tuning.xml` (服务端自动攒批与免记查询)
```xml
<clickhouse>
    <profiles>
        <default>
            <!-- 开启原生异步批量聚合写入：服务端自动在内存攒批，彻底解决客户端高并发微批写入产生的碎片爆炸 -->
            <async_insert>1</async_insert>
            <!-- 确保异步写入成功落盘后再响应客户端，保证数据落盘安全性与 Checkpoint 一致性 -->
            <wait_for_async_insert>1</wait_for_async_insert>
            <!-- 服务端内存攒批最大等待窗口（毫秒） -->
            <async_insert_busy_timeout_ms>200</async_insert_busy_timeout_ms>
            <!-- 服务端内存攒批最大字节（10MB） -->
            <async_insert_max_data_size>10485760</async_insert_max_data_size>
            <!-- 在数据采集管道中关闭微批查询日志记录，从源头切断 query_log 写入放大 -->
            <log_queries>0</log_queries>
            <parts_to_delay_insert>300</parts_to_delay_insert>
            <parts_to_throw_insert>600</parts_to_throw_insert>
            <max_delay_to_insert>1</max_delay_to_insert>
        </default>
    </profiles>
</clickhouse>
```

---

### Step 3: 检查并优化应用程序客户端写入代码

AI 必须检查后端应用（无论使用 Rust, Go, Python, Java, Node.js 等语言）向 ClickHouse 写入时的连接与选项：

1. **必须注入 `async_insert = 1` 与 `wait_for_async_insert = 1`**：
   - 即使服务端已配置，客户端连接层也应显式传递该参数，以确保在连接外部自建或云端 ClickHouse 时同样生效。
   - **参数说明**：
     - `async_insert = 1`：告诉服务端开启内存聚合。
     - `wait_for_async_insert = 1`：同步等待刷盘成功返回，**绝不改变应用端确认落盘后提交 Checkpoint / ACK 消息的事务逻辑**。
     - `async_insert_busy_timeout_ms = 200`：200ms 超时强制落盘。
2. **必须注入 `log_queries = 0`**：
   - 阻止数据管道的高频 `INSERT` 操作污染 `system.query_log`。
3. **连接串规范**：
   ```text
   http://user:password@clickhouse:8123/database?async_insert=1&wait_for_async_insert=1&async_insert_busy_timeout_ms=200&log_queries=0
   ```

---

### Step 4: 检查并优化 DDL 建表语句

AI 必须审查项目中的所有 ClickHouse 建表 SQL 文件（如 `schema.sql`、`migrations` 等）：

1. **放宽 MergeTree 表级容忍度**：
   所有 MergeTree 系列引擎建表语句末尾，必须追加或调整以下 `SETTINGS`：
   ```sql
   SETTINGS index_granularity = 8192,
            parts_to_delay_insert = 300,    -- 从默认 150 拓宽至 300
            parts_to_throw_insert = 600,    -- 从默认 300 拓宽至 600
            max_delay_to_insert = 1;        -- 单次最大延迟 1 秒，防客户端长挂
   ```
2. **严禁过度分区（PARTITION BY）**：
   - 检查单表总分区数是否预估会超过 **1,000**。
   - ❌ 严禁出现 `PARTITION BY toYYYYMMDD(time)`（按天分区跨度大时会爆炸）、`PARTITION BY user_id` 或 `PARTITION BY block_number` 等高基数字段。
   - ✅ 推荐按月 `PARTITION BY toYYYYMM(time)` 或粗粒度业务维度（如 `PARTITION BY tenant_id` / `chain_id`）。
3. **去除不必要的 `Nullable`**：
   - 检查是否有大量字段使用了 `Nullable(...)`。每个 Nullable 列都有额外开销与掩码文件，建议改用默认值（如数值 `0`、字符串 `''`）。
4. **定长数据改用 `FixedString`**：
   - 哈希、地址、UUID 优先采用 `FixedString(N)` 代替变长 `String`，节约内存且扫描更快。

---

### Step 5: 审查与排障常用 SQL 工具输出

当用户反映 ClickHouse 磁盘暴增或写入堵死卡住时，AI 必须优先给出以下诊断 SQL 指导用户排查：

1. **排查哪些表占用了磁盘（包括系统表）**：
   ```sql
   SELECT database, table,
          formatReadableSize(sum(data_compressed_bytes)) AS compressed,
          round(sum(data_uncompressed_bytes) / sum(data_compressed_bytes), 2) AS ratio,
          sum(rows) AS rows, count() AS parts
   FROM system.parts WHERE active = 1
   GROUP BY database, table ORDER BY sum(data_compressed_bytes) DESC;
   ```
2. **排查哪些表/分区 Parts 堆积超过 50 个（报警排查 Too Many Parts）**：
   ```sql
   SELECT database, table, partition, count() AS active_parts, sum(rows) AS total_rows
   FROM system.parts WHERE active = 1
   GROUP BY database, table, partition HAVING active_parts > 50
   ORDER BY active_parts DESC;
   ```
3. **磁盘爆满紧急一键救急（清空历史日志释放空间）**：
   ```sql
   TRUNCATE TABLE IF EXISTS system.query_log;
   TRUNCATE TABLE IF EXISTS system.part_log;
   TRUNCATE TABLE IF EXISTS system.trace_log;
   TRUNCATE TABLE IF EXISTS system.text_log;
   ```

---

## 3. AI 交付验收规范 (Verification Criteria)

AI 在完成检查或代码生成后，必须按照以下清单向用户自检汇报：
- [ ] Docker 配置文件中是否已补充 `ulimits.nofile: 262144`？
- [ ] Docker 配置文件中是否已补充容器日志轮转 `max-size: 50m`？
- [ ] 是否已配置 `<logger>` 文件日志级别为 `warning` 并启用 20M 轮转截断？
- [ ] 是否已创建并挂载 `clickhouse/config.d/system_logs.xml`（系统日志表截断为 1 天，text_log 限制为 warning 级别，移除 trace_log 与 processors_profile_log）？
- [ ] 是否已创建并挂载 `clickhouse/users.d/tuning.xml`（服务端启用 `async_insert=1`）？
- [ ] 应用程序客户端写入连接是否注入了 `async_insert=1`, `wait_for_async_insert=1`, `log_queries=0`？
- [ ] DDL 建表语句是否配置了 `parts_to_delay_insert = 300` 和 `parts_to_throw_insert = 600`？

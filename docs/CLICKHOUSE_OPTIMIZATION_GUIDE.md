# ClickHouse 生产级全维度调优与避坑指南 (Production Optimization Guide)

> **适用场景**：海量时序/事件日志分析、Web3 区块链数据湖、高并发流式写入、PB 级 OLAP 分析。  
> **核心原则**：ClickHouse 是**大批次低频写入的列式分析引擎**，非 OLTP 事务数据库。治水宜疏不宜堵，调优的核心是“控制碎片、抑制冗余日志、发挥向量化与压缩极致”。

---

## 目录
0. [Docker 容器化专属部署与调优范本 (Docker-First Best Practices)](#0-docker-容器化专属部署与调优范本)
1. [系统操作日志与磁盘空间治理 (System Logs Suppression)](#1-系统操作日志与磁盘空间治理)
2. [写入架构与碎片防堵 (Ingestion & Parts Merge Optimization)](#2-写入架构与碎片防堵)
3. [表结构与存储建模极致优化 (Schema & Storage Modeling)](#3-表结构与存储建模极致优化)
4. [查询性能与 SQL 编写黄金法则 (Query Optimization)](#4-查询性能与-sql-编写黄金法则)
5. [操作系统、内存与硬件配置调优 (OS & Hardware Tuning)](#5-操作系统内存与硬件配置调优)
6. [DBA 应急排障与常用监控 SQL 工具箱 (DBA Toolkit)](#6-dba-应急排障与常用监控-sql-工具箱)

---

## 0. Docker 容器化专属部署与调优范本

如果您在项目中使用 Docker / Docker Compose 运行 ClickHouse，**绝大部分配置无需手动修改容器内部文件或宿主机环境**，直接通过 Docker 的配置挂载、`ulimits` 和环境变量即可完全落地！

### 0.1 Docker 运行 ClickHouse 的 4 大专属致命陷阱
1. **文件句柄受限（`Too many open files`）**：
   Docker 默认分配给容器的 `nofile` 仅为 1024。ClickHouse 列式存储中每个列在每个 Part 都有独立的 `.bin` 和 `.mrk` 文件，高并发或多分片合并时会迅速消耗数千个文件句柄。**必须在 Docker 启动参数中显式注入 `--ulimit nofile=262144:262144`**。
2. **Docker RootFS（`/var/lib/docker/overlay2`）被撑爆**：
   若不挂载数据与日志目录，或者 Docker 守护进程未配置标准输出日志轮转，容器会将海量日志写在镜像读写层（Overlay2），无声无息耗尽系统根分区导致 Docker 守护进程瘫痪。
3. **内存 OOM 突发杀进程（Container Exit Code 137）**：
   ClickHouse 默认探测宿主机总内存并默认允许使用 90%（`max_server_memory_usage_to_ram_ratio = 0.9`）。如果 Docker 容器通过 `mem_limit` 限制了内存（如 8GB），但宿主机有 64GB，ClickHouse 会按 64GB*90% 申请内存，一旦超过 8GB 就会被内核 OOM Killer 强行 SIGKILL 击杀。
4. **进容器修改文件误区（易丢易碎）**：
   严禁通过 `docker exec` 进去修改 `/etc/clickhouse-server/config.xml`。官方镜像已内置标准扩展机制：**挂载宿主机目录到容器的 `/etc/clickhouse-server/config.d/` 和 `/etc/clickhouse-server/users.d/` 即可秒级覆盖合并，容器重建配置永不丢失**。

---

### 0.2 生产级 `docker-compose.yml` 完整开箱即用模板
这是可以直接复制到任何新项目的 ClickHouse Compose 服务定义：

```yaml
version: "3.8"

services:
  clickhouse:
    image: clickhouse/clickhouse-server:24.8
    container_name: clickhouse-server
    restart: unless-stopped
    # 1. 注入句柄数，彻底解决 Too many open files
    ulimits:
      nofile:
        soft: 262144
        hard: 262144
    environment:
      CLICKHOUSE_DB: my_database
      CLICKHOUSE_USER: my_user
      CLICKHOUSE_PASSWORD: my_password
      CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT: 1
    ports:
      - "8123:8123"   # HTTP REST 接口
      - "9000:9000"   # 原生 TCP 客户端接口
    # 2. 挂载持久化存储与调优配置 (最核心)
    volumes:
      # 数据与内部日志持久化至宿主机（建议位于 NVMe SSD）
      - ./data/clickhouse:/var/lib/clickhouse
      - ./logs/clickhouse:/var/log/clickhouse-server
      # 挂载自定义优化规则（系统日志截断、异步攒批、防堵阈值）
      - ./clickhouse/config.d:/etc/clickhouse-server/config.d:ro
      - ./clickhouse/users.d:/etc/clickhouse-server/users.d:ro
    # 3. 容器标准输出日志轮转，防止 Docker overlay2 撑爆磁盘
    logging:
      driver: "json-file"
      options:
        max-size: "50m"
        max-file: "3"
    # 4. 健康检查，等待就绪后再启动依赖服务
    healthcheck:
      test: ["CMD-SHELL", "wget -qO- http://127.0.0.1:8123/ping | grep -q Ok"]
      interval: 5s
      timeout: 5s
      retries: 20
```

---

### 0.3 单行 CLI `docker run` 极速启动命令
如果您不需要 Compose，使用单条命令行启动同样可以一键带入全部优化：

```bash
docker run -d \
  --name clickhouse \
  --restart unless-stopped \
  --ulimit nofile=262144:262144 \
  -p 8123:8123 -p 9000:9000 \
  -e CLICKHOUSE_DB=my_database \
  -e CLICKHOUSE_USER=my_user \
  -e CLICKHOUSE_PASSWORD=my_password \
  --log-opt max-size=50m --log-opt max-file=3 \
  -v $(pwd)/data/clickhouse:/var/lib/clickhouse \
  -v $(pwd)/logs/clickhouse:/var/log/clickhouse-server \
  -v $(pwd)/clickhouse/config.d:/etc/clickhouse-server/config.d:ro \
  -v $(pwd)/clickhouse/users.d:/etc/clickhouse-server/users.d:ro \
  clickhouse/clickhouse-server:24.8
```

---

### 0.4 进阶：一键构建“自带全部优化”的 ClickHouse 专属镜像 (零挂载)
如果您希望在其他项目中**连 `config.d` 目录都不想拷贝挂载**，可以直接制作一个自包含调优镜像：

**编写 `Dockerfile.clickhouse`**：
```dockerfile
FROM clickhouse/clickhouse-server:24.8

# 将优化配置直接打入镜像
COPY clickhouse/config.d/ /etc/clickhouse-server/config.d/
COPY clickhouse/users.d/ /etc/clickhouse-server/users.d/

# 声明标准数据目录卷
VOLUME ["/var/lib/clickhouse"]
```

构建并推送到您的私有 Registry（或本地）：
```bash
docker build -t my-company/clickhouse:24.8-optimized -f Dockerfile.clickhouse .
```
之后在任何服务器或项目中，只需运行 `docker run -d my-company/clickhouse:24.8-optimized`，天然免疫系统日志膨胀与 Parts 碎片堵塞，开箱即是生产级调优态！

---

---

## 1. 系统操作日志与磁盘空间治理

### 1.1 经典痛点
ClickHouse 默认在 `system` 数据库中记录所有系统行为（`query_log`, `part_log`, `trace_log`, `text_log`, `metric_log`），并且服务端输出到 `/var/log/clickhouse-server/clickhouse-server.log` 的日志级别默认为包含海量调试信息的 `<Trace>` / `<Debug>`。在持续高频写入场景下：
- **文本日志体积飞涨**：单日可能产生数 GB 的文本 Trace 日志，高频记录 Part 合并与内存采样的细枝末节。
- **系统审计表体积远超业务数据**：一条只有几十字节的 INSERT，在 `query_log`、`part_log` 和 `text_log` 中产生数 KB 的元数据。
- **默认保留期长**：默认保留 30 天，无运维干预时会在数周内耗尽数十甚至数百 GB 磁盘。

### 1.2 生产优化配置文件模板
在 ClickHouse 配置目录创建 `config.d/system_logs.xml` 并挂载至容器 `/etc/clickhouse-server/config.d/`：

```xml
<clickhouse>
    <!-- 0. 文件日志极致收敛：提升为 warning 级别（彻底过滤 trace/debug/information），限制单文件 20M，保留 1 份轮转 -->
    <logger>
        <level>warning</level>
        <log>/var/log/clickhouse-server/clickhouse-server.log</log>
        <errorlog>/var/log/clickhouse-server/clickhouse-server.err.log</errorlog>
        <size>20M</size>
        <count>1</count>
    </logger>

    <!-- 1. 收敛 query_log：保留期缩短至 1 天，调大刷新周期 -->
    <query_log>
        <database>system</database>
        <table>query_log</table>
        <partition_by>toYYYYMM(event_date)</partition_by>
        <flush_interval_milliseconds>7500</flush_interval_milliseconds>
        <max_size_rows>1048576</max_size_rows>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </query_log>

    <!-- 2. 彻底移除采样追踪 trace_log 与分析日志 processors_profile_log，生产环境极大节约磁盘 -->
    <trace_log remove="1"/>
    <processors_profile_log remove="1"/>
    <opentelemetry_span_log remove="1"/>

    <!-- 3. 收敛 part_log（数据分片与合并审计）：仅保留 1 天 -->
    <part_log>
        <database>system</database>
        <table>part_log</table>
        <partition_by>toYYYYMM(event_date)</partition_by>
        <flush_interval_milliseconds>7500</flush_interval_milliseconds>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </part_log>

    <!-- 4. text_log：仅保留 warning 及以上级别，且仅保留 1 天 -->
    <text_log>
        <level>warning</level>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </text_log>

    <!-- 5. metric_log / asynchronous_metric_log / asynchronous_insert_log：统一设为 1 天自动淘汰 -->
    <metric_log>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </metric_log>
    <asynchronous_metric_log>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </asynchronous_metric_log>
    <asynchronous_insert_log>
        <ttl>event_date + INTERVAL 1 DAY DELETE</ttl>
    </asynchronous_insert_log>

    <!-- 6. 后台合并（MergeTree Merge）线程池调优 -->
    <background_pool_size>16</background_pool_size>
</clickhouse>
```

### 1.3 客户端与用户级静音 (`log_queries = 0`)
在数据管道专属账号或写入连接中配置 `log_queries = 0`：
- **URL 参数**：`http://host:8123/?log_queries=0&...`
- **User Profile** (`users.d/tuning.xml`)：
  ```xml
  <clickhouse>
      <profiles>
          <default>
              <log_queries>0</log_queries>
          </default>
      </profiles>
  </clickhouse>
  ```
- **效果**：纯数据写入操作彻底不写入 `system.query_log`，零日志放大。

---

## 2. 写入架构与碎片防堵

### 2.1 经典痛点：`Too many parts (300)` 异常
ClickHouse 的 MergeTree 每次 INSERT 生成一个独立磁盘目录（Part），由后台 Merge 线程异步合并。
- **微批直写（Micro-batching）危害**：客户端每秒写入数十次小批次（1~10 行），Part 生成速度远超后台 Merge 速度。
- **阶梯式反压惩罚**：
  - 未合并 Part 达 `parts_to_delay_insert`（默认 150）：ClickHouse 强制休眠挂起写入请求，写入严重拖慢。
  - 未合并 Part 达 `parts_to_throw_insert`（默认 300）：ClickHouse 直接报错抛出 Code 252 异常，业务进程卡死或无限重试。

### 2.2 核心解法 A：服务端原生异步写入 (`async_insert`) —— 强烈推荐
ClickHouse 21.11+ 提供的杀手级特性，让 ClickHouse 在服务端内存中对微批数据自动攒批：

```text
客户端并发微批 INSERT (10~100 行)
              |
              v
[ ClickHouse 服务端内存 Buffer ] (缓冲 200ms 或达到 10MB)
              |
              v (原子落盘)
生成单个健康大 Part (数万行) -> 后台 Merge 极度轻松，零碎片堆积
```

#### 配置方式
1. **客户端连接设置**：
   - 开启异步攒批：`async_insert = 1`
   - 等待落盘确认：`wait_for_async_insert = 1`（保证数据落盘后才返回 HTTP 200，保证 Checkpoint/事务一致性）
   - 攒批窗口：`async_insert_busy_timeout_ms = 200`
   - 最大攒批大小：`async_insert_max_data_size = 10485760` (10MB)
2. **连接串示例**：
   `http://clickhouse:8123/default?async_insert=1&wait_for_async_insert=1&async_insert_busy_timeout_ms=200`

### 2.3 核心解法 B：MergeTree 表级参数提升容错度
在所有 MergeTree 建表语句末尾配置宽松参数：
```sql
CREATE TABLE my_table ( ... )
ENGINE = ReplacingMergeTree()
ORDER BY (id)
SETTINGS 
    index_granularity = 8192,
    parts_to_delay_insert = 300,    -- 放宽延迟阈值 (默认 150)
    parts_to_throw_insert = 600,    -- 放宽抛错阈值 (默认 300)
    max_delay_to_insert = 1;        -- 单次最大延迟 1 秒，防客户端长挂
```

### 2.4 客户端大批次法则
如果不使用 `async_insert`，应用层必须实现内存 Buffer：
- **写入批次**：单次写入 1,000 ~ 100,000 行。
- **写入频率**：每个表每秒写入不超过 1 ~ 2 次。

---

## 3. 表结构与存储建模极致优化

### 3.1 分区键设计 (`PARTITION BY`) —— 严禁过度分区
- **黄金准则**：单张表的总分区数量（Partitions）尽量控制在 **1,000 以内**，绝对不要超过 10,000。
- **常见错误**：
  - ❌ `PARTITION BY toYYYYMMDD(timestamp)`（按天分区，3 年产生 1,000+ 分区，如果写入跨日期，每次写入碎片乘倍）。
  - ❌ `PARTITION BY user_id` 或 `PARTITION BY block_number`（高基数字段分区，导致数十万分区，元数据撑爆内存，直接拖垮集群）。
- **推荐实践**：
  - 按月分区：`PARTITION BY toYYYYMM(timestamp)`（10 年仅 120 个分区）。
  - 粗粒度业务维度：`PARTITION BY chain_id`（区块链场景，每条链一个独立分区）。
  - 数据量极小（< 1000 万行）：可直接不分区（不写 `PARTITION BY`）。

### 3.2 排序键与主键 (`ORDER BY`) —— 核心查询加速器
ClickHouse 是稀疏索引（Sparse Index），默认每 8,192 行取一条作为索引节点。
- **排序键字段基数由低到高排列**：
  - 排序键最左侧放：**高频过滤、低基数（取值范围小）** 的字段（例如 `tenant_id`, `chain_id`, `status`）。
  - 中间放：**范围查询字段**（例如 `event_date`, `block_number`, `created_at`）。
  - 最右侧放：**高基数点查字段**（例如 `transaction_hash`, `user_id`）。
- **示例**：
  ```sql
  ORDER BY (chain_id, block_number, transaction_hash, log_index)
  ```

### 3.3 数据类型极致压缩与内存优化
1. **慎用 `Nullable`**：
   - `Nullable` 列在磁盘上需要额外的 Null 掩码文件（`*.null.bin`），会降低向量化执行速度并额外消耗 1 字节存储。
   - **替代方案**：使用默认值（数字用 `0`，字符串用 `''`，日期用 `1970-01-01`）。
2. **定长字符串优先用 `FixedString(N)`**：
   - 比如 EVM 地址（20 字节二进制或 42 字节十六进制）、MD5、SHA256、UUID。
   - `FixedString(42)` 比 `String` 省去变长长度前缀，扫描速度提升 20%~50%。
3. **列式压缩算法（Codecs）定制**：
   - 递增时间戳/高度：`CODEC(DoubleDelta, ZSTD)`（压缩比最高可提升数倍）。
   - 浮点数：`CODEC(Gorilla, ZSTD)`。
   - 稀疏高重复字符串：`CODEC(LowCardinality(String))`。
   - 普通文本：`CODEC(ZSTD(3))` 代替默认的 LZ4（若追求极高压缩比并接受轻微 CPU 损耗）。

### 3.4 二级跳数索引 (Data Skipping Index)
针对主键之外的高频过滤字段使用 Bloom Filter：
```sql
INDEX idx_topic0 topic0 TYPE bloom_filter(0.01) GRANULARITY 4,
INDEX idx_address address TYPE bloom_filter(0.01) GRANULARITY 4
```
- `0.01` 代表 1% 假阳性率。
- `GRANULARITY 4` 代表每 4 个 index_granularity（约 32,768 行）建立一个布隆过滤器块。

---

## 4. 查询性能与 SQL 编写黄金法则

### 4.1 ReplacingMergeTree 的 `FINAL` 性能陷阱
- **陷阱**：`SELECT * FROM table FINAL WHERE ...` 会强行在查询期将所有未合并的 Parts 在内存中解压并归并去重，多 Parts 场景下性能极差。
- **优化方案**：
  1. **带过滤条件后再 FINAL**：确保 `WHERE` 命中了主键前缀，先过滤分区再执行 FINAL。
  2. **聚合函数代替 FINAL (ArgMax 技巧)**：
     ```sql
     -- 性能比 FINAL 快 3~10 倍
     SELECT id, argMax(status, version) as latest_status
     FROM table
     WHERE chain_id = 1
     GROUP BY id
     ```
  3. **利用版本标记查询**：如果业务写入的是物理版本标记，查询时通过 `WHERE is_deleted = false` 结合过滤。

### 4.2 分页防深翻页 (Deep Paging)
- ❌ **严禁**：`LIMIT 100 OFFSET 1000000`（ClickHouse 需要解压并读取前 100 万行再抛弃，导致内存与 CPU 暴涨）。
- ✅ **推荐**：**Keyset Cursor 分页（游标分页）**：
  ```sql
  SELECT * FROM transactions
  WHERE chain_id = 1 
    AND (block_number, tx_index) < (18000000, 42)
  ORDER BY block_number DESC, tx_index DESC
  LIMIT 100
  ```

### 4.3 JOIN 优化原则
- ClickHouse 默认执行的是内存 **Hash Join**，右表会被全量拉入内存构建哈希表。
- **准则**：
  - **大表在左，小表在右**。
  - 严禁大表 JOIN 大表；海量分析场景下优先**建模为大宽表（Denormalization）**。
  - 低频变动维度表优先使用 **ClickHouse Dictionaries（外部字典）**，字典常驻内存，`dictGet()` 速度比 JOIN 快数量级。

### 4.4 内存熔断保护 (Prevent OOM)
为防止复杂或无界查询打崩整个服务器，设置全局或用户查询限制：
```xml
<profiles>
    <default>
        <!-- 单个查询最大内存限制 (例如 10GB) -->
        <max_memory_usage>10737418240</max_memory_usage>
        <!-- 超出内存限制时的行为：0-直接抛错异常，1-溢出到磁盘 -->
        <max_bytes_before_external_group_by>5368709120</max_bytes_before_external_group_by>
        <!-- 查询最大超时时间 (例如 60 秒) -->
        <max_execution_time>60</max_execution_time>
    </default>
</profiles>
```

---

## 5. 操作系统、内存与硬件配置调优

### 5.1 操作系统内核调优 (Linux OS)
ClickHouse 是极致榨干多核与 NVMe IO 的引擎，必须调整宿主机参数：

1. **关闭透明大页 (Transparent Huge Pages - THP)**：
   THP 会引发高延迟与严重的内存碎片：
   ```bash
   echo madvise | sudo tee /sys/kernel/mm/transparent_hugepage/enabled
   echo madvise | sudo tee /sys/kernel/mm/transparent_hugepage/defrag
   ```
2. **调大虚拟内存映射区**：
   ```bash
   sudo sysctl -w vm.max_map_count=2621440
   ```
3. **调低虚拟内存换页倾向 (Swappiness)**：
   ```bash
   sudo sysctl -w vm.swappiness=1
   ```
4. **调大打开文件描述符限制 (nofile)**：
   ClickHouse 每个列在每个 Part 都有两个文件（`.bin` 和 `.mrk`），表文件数极多：
   ```text
   # /etc/security/limits.d/clickhouse.conf
   clickhouse soft nofile 262144
   clickhouse hard nofile 262144
   ```

### 5.2 磁盘存储与冷热分层 (Storage Tiering)
- **热数据必须在 NVMe SSD 上**。
- **冷热分层存储（S3 / MinIO 归档）**：
  ClickHouse 支持原生多磁盘与存储策略，超过 30 天的数据自动冷迁移至对象存储（S3 / R2 / MinIO），本地仅留索引元数据：
  ```xml
  <clickhouse>
      <storage_configuration>
          <disks>
              <fast_ssd>
                  <path>/var/lib/clickhouse/</path>
              </fast_ssd>
              <s3_cold>
                  <type>s3</type>
                  <endpoint>https://s3.amazonaws.com/my-cold-bucket/data/</endpoint>
                  <access_key_id>AKIA...</access_key_id>
                  <secret_access_key>...</secret_access_key>
              </s3_cold>
          </disks>
          <policies>
              <hot_to_cold>
                  <volumes>
                      <hot><disk>fast_ssd</disk></hot>
                      <cold><disk>s3_cold</disk></cold>
                  </volumes>
                  <move_factor>0.2</move_factor>
              </hot_to_cold>
          </policies>
      </storage_configuration>
  </clickhouse>
  ```

---

## 6. DBA 应急排障与常用监控 SQL 工具箱

### 6.1 查看各数据库与数据表真实磁盘占用
```sql
SELECT
    database,
    table,
    formatReadableSize(sum(data_compressed_bytes)) AS compressed_size,
    formatReadableSize(sum(data_uncompressed_bytes)) AS uncompressed_size,
    round(sum(data_uncompressed_bytes) / sum(data_compressed_bytes), 2) AS ratio,
    sum(rows) AS total_rows,
    count() AS total_parts
FROM system.parts
WHERE active = 1
GROUP BY database, table
ORDER BY sum(data_compressed_bytes) DESC;
```

### 6.2 紧急排查：谁占用了大量碎片（检查是否面临 Too many parts）
```sql
SELECT
    database,
    table,
    partition,
    count() AS active_parts,
    sum(rows) AS total_rows,
    formatReadableSize(sum(bytes_on_disk)) AS size
FROM system.parts
WHERE active = 1
GROUP BY database, table, partition
HAVING active_parts > 50
ORDER BY active_parts DESC;
```

### 6.3 查看当前正在执行的慢查询与内存大户
```sql
SELECT
    query_id,
    user,
    elapsed,
    formatReadableSize(memory_usage) AS mem,
    formatReadableQuantity(read_rows) AS read_rows,
    formatReadableSize(read_bytes) AS read_bytes,
    query
FROM system.processes
ORDER BY elapsed DESC;
```

### 6.4 紧急杀掉卡住的慢查询
```sql
-- 通过上步查询获取 query_id
KILL QUERY WHERE query_id = 'xxxx-xxxx-xxxx';
```

### 6.5 查看后台当前正在进行的 Part 合并状态 (Merge Progress)
```sql
SELECT
    database,
    table,
    elapsed,
    progress,
    formatReadableSize(bytes_read_uncompressed) AS read,
    formatReadableSize(bytes_written_uncompressed) AS written,
    columns_written,
    memory_usage
FROM system.merges;
```

### 6.6 磁盘爆满应急一键释放命令
```sql
-- 1. 清理全部查询日志
TRUNCATE TABLE IF EXISTS system.query_log;
TRUNCATE TABLE IF EXISTS system.part_log;
TRUNCATE TABLE IF EXISTS system.trace_log;
TRUNCATE TABLE IF EXISTS system.text_log;

-- 2. 删除特定表的某个历史冷分区
ALTER TABLE my_db.my_table DROP PARTITION '202301';

-- 3. 手动触发大合并（注意：高 IO 操作，仅在空闲期使用）
OPTIMIZE TABLE my_db.my_table FINAL;
```

---

## 7. 快速自查清单 (ClickHouse Production Checklist)

- [ ] **系统日志与文件日志极致收敛**：是否配置了 `<logger>` 级别为 `warning` 并启用 20M 轮转截断？是否挂载了 `system_logs.xml` 限制系统表 TTL ≤ 1 天，text_log 限制为 warning 级别，并移除了 `trace_log`、`processors_profile_log`？
- [ ] **高频写入免记**：采集连接是否开启了 `log_queries = 0`？
- [ ] **异步批量写入**：微批写入是否启用了 `async_insert = 1` 与 `wait_for_async_insert = 1`？
- [ ] **表级防堵配置**：MergeTree 表是否声明了 `parts_to_delay_insert = 300` 与 `parts_to_throw_insert = 600`？
- [ ] **粗粒度分区**：单表总分区数是否严格控制在 1,000 以内（坚决不按小时/按块/按用户分区）？
- [ ] **字段规避 Nullable**：是否去除了不必要的 `Nullable` 改用业务默认值？
- [ ] **哈希定长优化**：固定长度哈希与地址是否使用了 `FixedString`？
- [ ] **查询规避全表 FINAL**：ReplacingMergeTree 查询是否优先使用 `argMax` 或限定主键前缀？
- [ ] **安全查询熔断**：用户 Profile 是否设置了 `max_memory_usage` 与 `max_execution_time`？
- [ ] **宿主机内核优化**：透明大页（THP）是否关闭？文件句柄数（nofile）是否 ≥ 262,144？

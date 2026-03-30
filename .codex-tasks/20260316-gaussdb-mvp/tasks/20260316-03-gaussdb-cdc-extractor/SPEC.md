# 子任务 3：GaussDB CDC Extractor 新建（single-full）

## Goal

- 实现 `GaussDB -> PG` 的 CDC 主路径：
  - 复制连接建立
  - 基于 `mppdb_decoding` 创建/启动逻辑复制槽
  - 拉取 WAL 输出并解析 JSON 事件
  - 将 `INSERT/UPDATE/DELETE` 转换为 `DtData::Dml(RowData)`，并推进 `Position::PgCdc` 位点
- 配置侧新增 `ExtractorConfig::GaussDBCdc`，并在 `dt-task` 中注册工厂方法

## Non-Goals

- 不承诺 DDL 捕获/回放（MVP 只覆盖 DML + 位点推进）
- 不新增新的 Position 类型（默认复用 `Position::PgCdc`）
- 不在本子任务交付 SHA256 认证支持

## Done-When

- `ExtractorConfig::GaussDBCdc` 可从 ini 配置解析
- `GaussDBJsonDecoder` 单测覆盖 INSERT/UPDATE/DELETE/BEGIN/COMMIT
- `cargo test -p dt-connector` 通过


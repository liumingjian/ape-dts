# GaussDB JDBC Replication Protocol Notes (Legacy, gaussdbjdbc.jar)

> Deprecated: prefer `raw/ref-gsjdbc4-replication-protocol.md`, which is extracted from the
> official driver `resources/gsjdbc4.jar` per current project requirement.

This note captures what we extracted from the locally available GaussDB JDBC driver to guide the Rust-side replication keepalive/status-update implementation.

## Source

- Jar: `~/.m2/repository/com/huawei/gaussdb/gaussdbjdbc/3.0/gaussdbjdbc-3.0.jar`
- Class: `com.huawei.gaussdb.jdbc.core.v3.replication.V3PGReplicationStream`
- Extraction command:
  - `javap -classpath <jar> -c -private com.huawei.gaussdb.jdbc.core.v3.replication.V3PGReplicationStream`

## `StandbyStatusUpdate` (client -> server)

From `prepareUpdateStatus(...)`, the payload written to the COPY stream is:

- Total length: **65 bytes**
- Byte order: **Little-endian**
- Layout (in order):
  1. `u8` tag: `'r'` (0x72)
  2. `i64` **Long.MAX_VALUE** (`9223372036854775807`)
  3. `i64` received LSN (`LogSequenceNumber.asLong()`)
  4. `i64` flushed LSN
  5. `i64` **Long.MAX_VALUE**
  6. `i64` applied LSN
  7. `i32` **Integer.MAX_VALUE** (`2147483647`)
  8. `i32` **Integer.MAX_VALUE**
  9. `i64` clock (driver-computed; treated as microseconds-based in logs)
  10. `u8` reply flag:
      - `1` when `forceUpdateStatus()` path is used
      - otherwise `1` only if received LSN is `INVALID_LSN`, else `0`
  11. `i32` `0`
  12. `u8` `1`
  13. `u8` `1`
  14. `u8` `1`

## `Keepalive` (server -> client)

From `processKeepAliveMessage(ByteBuffer)`:

- The keepalive buffer is parsed as **Little-endian**.
- Fields:
  1. `i64` server LSN
  2. `i32` (unused)
  3. `i32` (unused)
  4. `i64` clock
  5. `u8` need-reply flag

These fields informed the Rust-side `GaussDBCdcExtractor` keepalive parsing + status update encoding.

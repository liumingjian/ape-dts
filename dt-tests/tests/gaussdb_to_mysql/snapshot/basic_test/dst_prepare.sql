DROP DATABASE IF EXISTS gaussdb_to_mysql_snapshot_dst;
CREATE DATABASE gaussdb_to_mysql_snapshot_dst;

CREATE TABLE gaussdb_to_mysql_snapshot_dst.gaussdb_to_mysql_snapshot_basic (
  id INTEGER PRIMARY KEY,
  val TEXT
);

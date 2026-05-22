DROP DATABASE IF EXISTS gaussdb_to_mysql_cdc_dst;
CREATE DATABASE gaussdb_to_mysql_cdc_dst;
CREATE TABLE gaussdb_to_mysql_cdc_dst.gaussdb_to_mysql_cdc_basic (
  id INTEGER PRIMARY KEY,
  val TEXT
);

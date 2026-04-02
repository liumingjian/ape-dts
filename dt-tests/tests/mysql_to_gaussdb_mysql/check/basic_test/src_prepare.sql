DROP DATABASE IF EXISTS gaussdb_mysql_check_basic;

CREATE DATABASE IF NOT EXISTS gaussdb_mysql_check_basic DEFAULT CHARACTER SET utf8mb4;

CREATE TABLE gaussdb_mysql_check_basic.check_basic (
  id INT PRIMARY KEY,
  val VARCHAR(32)
);

DROP DATABASE IF EXISTS gaussdb_mysql_struct_basic;

CREATE DATABASE IF NOT EXISTS gaussdb_mysql_struct_basic DEFAULT CHARACTER SET utf8mb4;

CREATE TABLE gaussdb_mysql_struct_basic.struct_basic (
  id INT NOT NULL,
  name VARCHAR(32) NOT NULL COMMENT 'name_comment',
  amount DECIMAL(10,2) DEFAULT 0.00,
  PRIMARY KEY (id),
  KEY idx_name(name)
) COMMENT='struct_basic_comment';

DROP DATABASE IF EXISTS gaussdb_mysql_cdc_basic;
CREATE DATABASE gaussdb_mysql_cdc_basic;

CREATE TABLE gaussdb_mysql_cdc_basic.cdc_basic (
    id INT PRIMARY KEY,
    val VARCHAR(64)
);

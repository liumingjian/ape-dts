DROP DATABASE IF EXISTS gaussdb_mysql_cdc_failover;
CREATE DATABASE gaussdb_mysql_cdc_failover;

CREATE TABLE gaussdb_mysql_cdc_failover.cdc_failover (
    id INT PRIMARY KEY,
    val VARCHAR(64)
);


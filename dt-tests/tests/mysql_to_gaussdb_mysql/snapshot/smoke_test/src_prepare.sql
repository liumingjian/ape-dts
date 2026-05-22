DROP DATABASE IF EXISTS gaussdb_mysql_smoke;
CREATE DATABASE gaussdb_mysql_smoke;

CREATE TABLE gaussdb_mysql_smoke.smoke_basic (
    id INT PRIMARY KEY,
    name VARCHAR(32),
    amount DECIMAL(10,2),
    updated_at DATETIME
);

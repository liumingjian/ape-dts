DROP DATABASE IF EXISTS gaussdb_mysql_cdc_type_matrix;
CREATE DATABASE gaussdb_mysql_cdc_type_matrix;

CREATE TABLE gaussdb_mysql_cdc_type_matrix.type_matrix (
    id INT PRIMARY KEY,
    c_int INT,
    c_big BIGINT,
    c_dec DECIMAL(10,2),
    c_double DOUBLE,
    c_varchar VARCHAR(64),
    c_text TEXT,
    c_datetime DATETIME,
    c_timestamp TIMESTAMP NULL,
    c_date DATE,
    c_time TIME,
    c_json JSON
);


DROP DATABASE IF EXISTS gaussdb_mysql_cdc_resume;
CREATE DATABASE gaussdb_mysql_cdc_resume;

CREATE TABLE gaussdb_mysql_cdc_resume.cdc_resume (
    id INT PRIMARY KEY,
    val VARCHAR(64)
);


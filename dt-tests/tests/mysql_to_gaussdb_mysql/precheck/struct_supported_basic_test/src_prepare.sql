DROP DATABASE IF EXISTS gaussdb_mysql_precheck_basic;

CREATE DATABASE gaussdb_mysql_precheck_basic;

CREATE TABLE gaussdb_mysql_precheck_basic.table_test_1(id integer, text varchar(10), primary key (id));

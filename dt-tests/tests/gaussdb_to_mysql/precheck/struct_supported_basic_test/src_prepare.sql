DROP SCHEMA IF EXISTS gaussdb_to_mysql_precheck_basic CASCADE;
CREATE SCHEMA gaussdb_to_mysql_precheck_basic;
CREATE TABLE gaussdb_to_mysql_precheck_basic.table_test_1(id integer, val varchar(10), primary key (id));


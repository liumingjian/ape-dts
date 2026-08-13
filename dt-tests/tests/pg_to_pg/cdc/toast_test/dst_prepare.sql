DROP SCHEMA IF EXISTS test_db_1 CASCADE;

CREATE SCHEMA test_db_1;

CREATE TABLE test_db_1.toast_table ( f_0 serial, f_1 text, f_2 int, PRIMARY KEY (f_0) );

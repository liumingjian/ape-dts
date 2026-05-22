UPDATE gaussdb_mysql_cdc_failover.cdc_failover SET val = 'bb' WHERE id = 2;
DELETE FROM gaussdb_mysql_cdc_failover.cdc_failover WHERE id = 1;
INSERT INTO gaussdb_mysql_cdc_failover.cdc_failover VALUES (3, 'c');


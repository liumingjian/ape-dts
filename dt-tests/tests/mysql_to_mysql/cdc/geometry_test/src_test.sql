INSERT INTO test_db_1.geometry_test VALUES (NULL, ST_GeomFromText('POINT(1 1)'), ST_GeomFromText('POINT(2 2)'), ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'), 1);
INSERT INTO test_db_1.geometry_test VALUES (NULL, ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'), ST_GeomFromText('POINT(-3.5 4.25)'), NULL, 2);

UPDATE test_db_1.geometry_test SET f_2 = ST_GeomFromText('POINT(9 9)') WHERE f_0 = 1;
UPDATE test_db_1.geometry_test SET f_4 = 20 WHERE f_0 = 2;

DELETE FROM test_db_1.geometry_test WHERE f_0 = 2;

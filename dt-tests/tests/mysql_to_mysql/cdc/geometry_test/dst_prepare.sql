DROP DATABASE IF EXISTS test_db_1;

CREATE DATABASE test_db_1;

-- f_1 / f_2 are NOT NULL on purpose: a spatial value that is silently dropped on the way
-- (the old snapshot path wrote NULL for unmodelled types) fails the insert instead of
-- quietly matching a NULL on both sides.
CREATE TABLE test_db_1.geometry_test(f_0 INT AUTO_INCREMENT, f_1 GEOMETRY NOT NULL, f_2 POINT NOT NULL, f_3 POLYGON, f_4 INT, PRIMARY KEY(f_0));

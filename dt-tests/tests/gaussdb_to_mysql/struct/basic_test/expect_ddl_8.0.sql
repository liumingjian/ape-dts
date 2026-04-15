gaussdb_to_mysql_struct_dst
CREATE DATABASE `gaussdb_to_mysql_struct_dst` /*!40100 DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci */ /*!80016 DEFAULT ENCRYPTION='N' */

gaussdb_to_mysql_struct_dst.gaussdb_to_mysql_struct_basic
CREATE TABLE `gaussdb_to_mysql_struct_basic` (
  `id` int NOT NULL,
  `val` varchar(10) DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci

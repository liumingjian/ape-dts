gaussdb_to_mysql_struct_adv_dst
CREATE DATABASE `gaussdb_to_mysql_struct_adv_dst` /*!40100 DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci */ /*!80016 DEFAULT ENCRYPTION='N' */

gaussdb_to_mysql_struct_adv_dst.gaussdb_to_mysql_struct_advanced
CREATE TABLE `gaussdb_to_mysql_struct_advanced` (
  `id1` int NOT NULL,
  `id2` int NOT NULL,
  `val` varchar(10) NOT NULL DEFAULT 'x',
  `amount` decimal(10,2) DEFAULT '0.00',
  `flag` tinyint(1) DEFAULT '1',
  `created_at` datetime DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (`id1`,`id2`),
  UNIQUE KEY `gaussdb_to_mysql_struct_advanced_val_uk` (`val`),
  KEY `gaussdb_to_mysql_struct_advanced_amount_idx` (`amount`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci


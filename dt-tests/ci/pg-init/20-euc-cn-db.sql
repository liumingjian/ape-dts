-- `pg_to_pg/snapshot/charset_euc_cn_test` (and the two resume-from-* charset variants) connect
-- to a hardcoded `euc_cn_db` — the database is deliberately not created by the test's own
-- prepare SQL, because CREATE DATABASE cannot run inside the transaction the runner uses, and
-- the encoding has to be fixed at creation time. README documents it as a manual step; in CI it
-- has to exist before the suite starts, so it is created here, once, at initdb time.
CREATE DATABASE euc_cn_db
  ENCODING 'EUC_CN'
  LC_COLLATE 'C'
  LC_CTYPE 'C'
  TEMPLATE template0;

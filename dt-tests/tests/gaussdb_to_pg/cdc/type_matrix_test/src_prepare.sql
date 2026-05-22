DROP TABLE IF EXISTS public.gaussdb_cdc_type_matrix;
CREATE TABLE public.gaussdb_cdc_type_matrix (
  id INTEGER PRIMARY KEY,
  ts_col smalldatetime,
  tiny_col tinyint,
  nvarchar_col nvarchar2(32),
  clob_col clob,
  blob_col blob
);

INSERT INTO public.gaussdb_type_matrix_snapshot (
  id, ts_col, tiny_col, nvarchar_col, clob_col, blob_col
) VALUES
  (1, '2026-04-02 13:20:00', 7, 'alpha', 'first clob text', '\x0001ff'),
  (2, NULL, NULL, 'beta', NULL, NULL);

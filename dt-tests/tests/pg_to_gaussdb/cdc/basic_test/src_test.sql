INSERT INTO public.pg_to_gaussdb_cdc_basic (id, val) VALUES
  (1, 'a'),
  (2, 'b');

UPDATE public.pg_to_gaussdb_cdc_basic SET val = 'c' WHERE id = 2;

DELETE FROM public.pg_to_gaussdb_cdc_basic WHERE id = 1;


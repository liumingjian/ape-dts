INSERT INTO public.gaussdb_oracle_to_pg_cdc_basic (id, val) VALUES (1, 'a'), (2, 'b');
UPDATE public.gaussdb_oracle_to_pg_cdc_basic SET val = 'bb' WHERE id = 2;
DELETE FROM public.gaussdb_oracle_to_pg_cdc_basic WHERE id = 1;


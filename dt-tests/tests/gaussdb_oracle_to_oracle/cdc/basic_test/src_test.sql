INSERT INTO public.gdbo_ora_cdc_basic (id, val) VALUES (1, 'a'), (2, 'b');
UPDATE public.gdbo_ora_cdc_basic SET val = 'bb' WHERE id = 2;
DELETE FROM public.gdbo_ora_cdc_basic WHERE id = 1;


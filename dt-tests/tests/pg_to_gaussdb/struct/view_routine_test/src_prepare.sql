DROP MATERIALIZED VIEW IF EXISTS public.gaussdb_struct_vr_matview;
DROP VIEW IF EXISTS public.gaussdb_struct_vr_view;
DROP PROCEDURE IF EXISTS public.gaussdb_struct_vr_proc();
DROP FUNCTION IF EXISTS public.gaussdb_struct_vr_func();
DROP TABLE IF EXISTS public.gaussdb_struct_vr_base;

CREATE TABLE public.gaussdb_struct_vr_base (
  id INTEGER PRIMARY KEY,
  val TEXT
);

CREATE OR REPLACE FUNCTION public.gaussdb_struct_vr_func()
RETURNS integer
LANGUAGE sql
AS $$ SELECT 42 $$;

CREATE OR REPLACE PROCEDURE public.gaussdb_struct_vr_proc()
LANGUAGE plpgsql
AS $proc$
BEGIN
  PERFORM 1;
END;
$proc$;

CREATE VIEW public.gaussdb_struct_vr_view AS
SELECT id, val FROM public.gaussdb_struct_vr_base;

CREATE MATERIALIZED VIEW public.gaussdb_struct_vr_matview AS
SELECT id, val FROM public.gaussdb_struct_vr_base
WITH NO DATA;

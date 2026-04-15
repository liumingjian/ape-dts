DROP TABLE IF EXISTS public.gaussdb_to_mysql_struct_advanced;
CREATE TABLE public.gaussdb_to_mysql_struct_advanced (
  id1 integer NOT NULL,
  id2 integer NOT NULL,
  val varchar(10) NOT NULL DEFAULT 'x',
  amount numeric(10,2) DEFAULT 0,
  flag boolean DEFAULT true,
  created_at timestamp without time zone DEFAULT now(),
  PRIMARY KEY (id1, id2)
);
CREATE UNIQUE INDEX gaussdb_to_mysql_struct_advanced_val_uk ON public.gaussdb_to_mysql_struct_advanced (val);
CREATE INDEX gaussdb_to_mysql_struct_advanced_amount_idx ON public.gaussdb_to_mysql_struct_advanced (amount);


import { describe, expect, it } from "vitest";
import { buildRouterConfig } from "@/utils/routerConfig";

describe("buildRouterConfig", () => {
  it("normalizes table mappings with the first colon as separator", () => {
    expect(
      buildRouterConfig(
        undefined,
        " APE_DTS.T1 : public.t1 ",
        undefined,
        undefined,
        false,
      ),
    ).toEqual({ tb_map: "APE_DTS.T1:public.t1" });
  });

  it("converts per-column mappings to the JSON col_map contract", () => {
    const router = buildRouterConfig(
      undefined,
      undefined,
      [
        "APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE.ID:public.t_oracle_to_gaussdb_oracle.id",
        "APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE.TRACER:public.t_oracle_to_gaussdb_oracle.tracer",
        "APE_DTS.T_ORACLE_TO_GAUSSDB_ORACLE.PAYLOAD:public.t_oracle_to_gaussdb_oracle.payload",
      ].join("\n"),
      undefined,
      false,
    );

    expect(router?.col_map).toBe(
      '[{"db":"APE_DTS","tb":"T_ORACLE_TO_GAUSSDB_ORACLE","col_map":{"ID":"id","TRACER":"tracer","PAYLOAD":"payload"}}]',
    );
  });

  it("preserves an explicit JSON col_map", () => {
    const colMap =
      'json:[{"db":"public","tb":"t1","col_map":{"id":"ID"}}]';
    expect(
      buildRouterConfig(undefined, undefined, colMap, undefined, false),
    ).toEqual({ col_map: colMap });
  });

  it("includes topic_map only for Kafka targets", () => {
    expect(
      buildRouterConfig(undefined, undefined, undefined, "db.t:topic", true),
    ).toEqual({ topic_map: "db.t:topic" });
    expect(
      buildRouterConfig(undefined, undefined, undefined, "db.t:topic", false),
    ).toBeUndefined();
  });
});

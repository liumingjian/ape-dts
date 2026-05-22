import { describe, expect, it } from "vitest";
import type {
  CreateTaskDto,
  TaskCategory,
  SyncMode,
  ExtractType,
  EngineType,
} from "@/types/domain";

const SNAPSHOT_CDC_SUPPORTED_ENGINES: EngineType[] = [
  "mysql",
  "postgres",
  "oracle",
  "gaussdb",
];

function syncModeToExtractType(
  mode: SyncMode,
  cat: TaskCategory,
  sourceEngine: EngineType,
): ExtractType {
  if (cat === "struct") return "struct";
  if (cat === "check") return "snapshot";
  if (mode === "snapshot_cdc") {
    if (!SNAPSHOT_CDC_SUPPORTED_ENGINES.includes(sourceEngine)) {
      throw new Error(
        "Snapshot+CDC mode is currently only supported for MySQL sources",
      );
    }
    return "snapshot_and_cdc";
  }
  if (mode === "snapshot") return "snapshot";
  if (mode === "cdc") return "cdc";
  return "snapshot";
}

function makeEndpoint(dsn: string) {
  return { url: dsn };
}

function normalizeDbFilter(value?: string, tableFilter?: string): string {
  const trimmed = value?.trim() || "";
  const tableTrimmed = tableFilter?.trim() || "";
  if (tableTrimmed && (!trimmed || trimmed === "*")) return "";
  if (trimmed) return trimmed;
  return "*";
}

const SRC_DSN = "SRC_DSN_PLACEHOLDER";
const TGT_DSN = "TGT_DSN_PLACEHOLDER";
const GAUSS_DSN = "GAUSS_DSN_PLACEHOLDER";

describe("WizardForm → CreateTaskDto transformation", () => {
  describe("syncModeToExtractType", () => {
    it('maps struct category to "struct" extract type', () => {
      expect(syncModeToExtractType("snapshot", "struct", "mysql")).toBe(
        "struct",
      );
    });

    it('maps check category to "snapshot" extract type', () => {
      expect(syncModeToExtractType("snapshot_cdc", "check", "mysql")).toBe(
        "snapshot",
      );
    });

    it('maps snapshot syncMode to "snapshot" extract type', () => {
      expect(syncModeToExtractType("snapshot", "snapshot", "mysql")).toBe(
        "snapshot",
      );
    });

    it('maps cdc syncMode to "cdc" extract type', () => {
      expect(syncModeToExtractType("cdc", "cdc", "mysql")).toBe("cdc");
    });

    it('maps snapshot_cdc syncMode to "snapshot_and_cdc" when source is MySQL', () => {
      expect(syncModeToExtractType("snapshot_cdc", "snapshot", "mysql")).toBe(
        "snapshot_and_cdc",
      );
    });

    it("allows snapshot_cdc for Oracle sources", () => {
      expect(syncModeToExtractType("snapshot_cdc", "snapshot", "oracle")).toBe(
        "snapshot_and_cdc",
      );
    });

    it("allows snapshot_cdc for GaussDB sources", () => {
      expect(syncModeToExtractType("snapshot_cdc", "snapshot", "gaussdb")).toBe(
        "snapshot_and_cdc",
      );
    });

    it("allows snapshot_cdc for PostgreSQL sources", () => {
      expect(
        syncModeToExtractType("snapshot_cdc", "snapshot", "postgres"),
      ).toBe("snapshot_and_cdc");
    });

    it("throws an explicit error for snapshot_cdc on unsupported source engines (no silent downgrade)", () => {
      expect(() =>
        syncModeToExtractType("snapshot_cdc", "snapshot", "mongo"),
      ).toThrow(/only supported for MySQL/i);
    });
  });

  describe("CreateTaskDto shape (wire format)", () => {
    it("contains all required fields for a snapshot task", () => {
      const dto: CreateTaskDto = {
        name: "test-snapshot",
        kind: "snapshot",
        engineSource: "mysql",
        engineTarget: "mysql",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "snapshot" },
        sinker: {},
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        runtime: { sync_schema: true, sync_index: false },
        resourceGroupId: "default",
      };
      expect(dto.kind).toBe("snapshot");
      expect(dto.extractor.extract_type).toBe("snapshot");
      expect(dto.engineSource).toBe("mysql");
      expect(dto.engineTarget).toBe("mysql");
      expect(dto.parallelizer.parallel_size).toBe(4);
      expect(dto.runtime?.sync_schema).toBe(true);
      expect(dto.runtime?.sync_index).toBe(false);
    });

    it("contains snapshot_and_cdc extract_type for MySQL snapshot+cdc tasks", () => {
      const dto: CreateTaskDto = {
        name: "test-snapshot-cdc",
        kind: "snapshot",
        engineSource: "mysql",
        engineTarget: "mysql",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "snapshot_and_cdc" },
        sinker: {},
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.extractor.extract_type).toBe("snapshot_and_cdc");
      expect(dto.engineSource).toBe("mysql");
    });

    it("includes GaussDB subMode when source is gaussdb", () => {
      const dto: CreateTaskDto = {
        name: "gaussdb-cdc",
        kind: "cdc",
        engineSource: "gaussdb",
        engineTarget: "mysql",
        subMode: "pg-mode",
        sourceEndpoint: makeEndpoint(GAUSS_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "cdc" },
        sinker: {},
        parallelizer: { parallel_type: "rdb_merge", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.subMode).toBe("pg-mode");
      expect(dto.extractor.extract_type).toBe("cdc");
    });

    it("carries GaussDB candidate hosts for RW primary discovery and failover", () => {
      const dto: CreateTaskDto = {
        name: "gaussdb-ha-cdc",
        kind: "cdc",
        engineSource: "gaussdb",
        engineTarget: "mysql",
        subMode: "oracle-mode",
        sourceEndpoint: {
          url: GAUSS_DSN,
          candidateHosts: ["10.250.0.157:8000", "10.250.0.223:8000"],
        },
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "cdc" },
        sinker: {},
        parallelizer: { parallel_type: "rdb_merge", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.sourceEndpoint.candidateHosts).toEqual([
        "10.250.0.157:8000",
        "10.250.0.223:8000",
      ]);
    });

    it("keeps source and target GaussDB sub modes separate", () => {
      const dto: CreateTaskDto = {
        name: "oracle-to-gaussdb-oracle",
        kind: "snapshot",
        engineSource: "oracle",
        engineTarget: "gaussdb",
        targetSubMode: "oracle-mode",
        subMode: "oracle-mode",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(GAUSS_DSN),
        extractor: { extract_type: "snapshot_and_cdc" },
        sinker: {},
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.sourceSubMode).toBeUndefined();
      expect(dto.targetSubMode).toBe("oracle-mode");
    });

    it("includes filter with selected objects", () => {
      const dto: CreateTaskDto = {
        name: "with-filter",
        kind: "snapshot",
        engineSource: "mysql",
        engineTarget: "mysql",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "snapshot" },
        sinker: {},
        filter: {
          do_dbs: "app_db",
          do_tbs: "app_db.users,app_db.orders",
        },
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.filter?.do_dbs).toBe("app_db");
      expect(dto.filter?.do_tbs).toBe("app_db.users,app_db.orders");
    });

    it("does not combine wildcard do_dbs with explicit do_tbs", () => {
      expect(normalizeDbFilter("", "public.t_gaussdb_oracle_to_oracle")).toBe(
        "",
      );
      expect(normalizeDbFilter("*", "public.t_gaussdb_oracle_to_oracle")).toBe(
        "",
      );
      expect(normalizeDbFilter("", "")).toBe("*");
      expect(normalizeDbFilter("app_db", "app_db.users")).toBe("app_db");
    });

    it("includes processor with lua_code for inline Lua", () => {
      const dto: CreateTaskDto = {
        name: "with-lua-inline",
        kind: "snapshot",
        engineSource: "mysql",
        engineTarget: "mysql",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "snapshot" },
        sinker: {},
        processor: {
          lua_code_file: "inline",
          lua_code: "function process() end",
        },
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.processor?.lua_code_file).toBe("inline");
      expect(dto.processor?.lua_code).toBe("function process() end");
    });

    it("includes processor with lua_code for file upload Lua", () => {
      const dto: CreateTaskDto = {
        name: "with-lua-file",
        kind: "snapshot",
        engineSource: "mysql",
        engineTarget: "mysql",
        sourceEndpoint: makeEndpoint(SRC_DSN),
        targetEndpoint: makeEndpoint(TGT_DSN),
        extractor: { extract_type: "snapshot" },
        sinker: {},
        processor: {
          lua_code_file: "inline",
          lua_code: "-- file content here\nfunction process() end",
        },
        parallelizer: { parallel_type: "snapshot", parallel_size: 4 },
        pipeline: {
          buffer_size: 16000,
          checkpoint_interval_secs: 10,
          max_rps: 0,
        },
        resumer: { resume_type: "from_log" },
        resourceGroupId: "default",
      };
      expect(dto.processor?.lua_code_file).toBe("inline");
      expect(dto.processor?.lua_code).toContain("function process() end");
    });
  });
});

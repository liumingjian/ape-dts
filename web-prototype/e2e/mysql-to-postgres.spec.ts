import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { promisify } from "node:util";

import { expect, test } from "@playwright/test";

const execFileAsync = promisify(execFile);

const ADMIN = { username: "admin", password: "admin123" } as const;
const API = "http://127.0.0.1:8080/api";
const MYSQL_DB = "test_db";
const MYSQL_USER = "root";
const MYSQL_PASS = "123456";
const SOURCE_DSN = `mysql://${MYSQL_USER}:${MYSQL_PASS}@127.0.0.1:3307/${MYSQL_DB}?ssl-mode=disabled`;
const TARGET_DSN =
  "postgres://postgres:postgres@127.0.0.1:5434/postgres?options[statement_timeout]=10s";
const TERMINAL_STATUSES = ["stopped", "completed", "failed"] as const;
const SUCCESS_STATUSES = ["stopped", "completed"] as const;
const LICENSE_SECRET = "ape-dts-console-license-secret-2025";

type AuthCookies = {
  readonly cookieHeader: string;
  readonly xsrfToken: string;
};

type MigrationCase = {
  readonly schemaName: string;
  readonly tableName: string;
  readonly tracer: string;
  readonly payload: string;
};

type TaskResponse = {
  readonly id?: unknown;
  readonly status?: unknown;
};

type RunResponse = {
  readonly items?: unknown;
};

type RunItem = {
  readonly id?: unknown;
  readonly runId?: unknown;
  readonly run_id?: unknown;
  readonly status?: unknown;
  readonly exitCode?: unknown;
  readonly exit_code?: unknown;
  readonly exitStatus?: unknown;
  readonly exit_status?: unknown;
};

type ApiRequest = {
  readonly path: string;
  readonly method: string;
  readonly auth: AuthCookies;
  readonly body?: unknown;
};

async function apiLogin(): Promise<AuthCookies> {
  const res = await fetch(`${API}/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(ADMIN),
    redirect: "manual",
  });
  const body = await res.text();
  expect(res.status, `API login failed: ${res.status} ${body}`).toBe(200);
  const setCookies = res.headers.getSetCookie() ?? [];
  const cookieHeader = setCookies
    .map((cookie) => cookie.split(";")[0])
    .join("; ");
  const xsrfCookie = setCookies.find((cookie) =>
    cookie.startsWith("XSRF-TOKEN="),
  );
  const xsrfToken = xsrfCookie
    ? decodeURIComponent(xsrfCookie.split("=")[1]?.split(";")[0] ?? "")
    : "";
  expect(cookieHeader, "API login did not return session cookies").not.toBe("");
  expect(xsrfToken, "API login did not return XSRF-TOKEN").not.toBe("");
  return { cookieHeader, xsrfToken };
}

async function authedFetch(request: ApiRequest): Promise<Response> {
  return fetch(`${API}${request.path}`, {
    method: request.method,
    headers: {
      "Content-Type": "application/json",
      Cookie: request.auth.cookieHeader,
      "X-XSRF-TOKEN": request.auth.xsrfToken,
    },
    body: request.body === undefined ? undefined : JSON.stringify(request.body),
  });
}

function createActivationCode(maxTasks: number): string {
  const sku = "professional";
  const expireAt = "2099-12-31";
  const grantedTo = "mysql-postgres-e2e";
  const sig = createHash("sha256")
    .update(`${sku}:${maxTasks}:${expireAt}:${grantedTo}:${LICENSE_SECRET}`)
    .digest("hex")
    .slice(0, 16);
  return Buffer.from(
    JSON.stringify({
      sku,
      maxTasks,
      expireAt,
      grantedTo,
      sig,
    }),
  ).toString("base64url");
}

async function activateLicense(auth: AuthCookies): Promise<void> {
  const res = await authedFetch({
    path: "/license/activate",
    method: "POST",
    auth,
    body: { code: createActivationCode(100) },
  });
  const bodyText = await res.text();
  expect(
    res.status,
    `License activation failed: ${res.status} ${bodyText}`,
  ).toBe(200);
}

async function runDocker(
  container: string,
  args: readonly string[],
  timeoutMs = 20_000,
): Promise<string> {
  const { stdout, stderr } = await execFileAsync(
    "docker",
    ["exec", container, ...args],
    {
      timeout: timeoutMs,
    },
  );
  const output = `${stdout}${stderr}`;
  return output.trim();
}

function sqlLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

function newMigrationCase(): MigrationCase {
  const suffix = Date.now().toString(36);
  return {
    schemaName: `e2e_mysql_pg_${suffix}`,
    tableName: `orders_${suffix}`,
    tracer: `tracer_${suffix}`,
    payload: `payload_${suffix}`,
  };
}

function parseTaskResponse(bodyText: string): TaskResponse {
  const parsed: unknown = JSON.parse(bodyText);
  if (!parsed || typeof parsed !== "object") {
    throw new Error(`Expected task response object, got: ${bodyText}`);
  }
  return parsed;
}

function parseRunResponse(bodyText: string): RunResponse {
  const parsed: unknown = JSON.parse(bodyText);
  if (!parsed || typeof parsed !== "object") {
    throw new Error(`Expected run response object, got: ${bodyText}`);
  }
  return parsed;
}

function isRunItem(value: unknown): value is RunItem {
  return !!value && typeof value === "object";
}

async function seedSourceRow(migration: MigrationCase): Promise<void> {
  const sql = [
    `DROP TABLE IF EXISTS \`${migration.schemaName}\`.\`${migration.tableName}\``,
    `DROP DATABASE IF EXISTS \`${migration.schemaName}\``,
    `CREATE DATABASE \`${migration.schemaName}\``,
    `CREATE TABLE \`${migration.schemaName}\`.\`${migration.tableName}\` (` +
      "id INT PRIMARY KEY, tracer VARCHAR(128) NOT NULL, payload VARCHAR(256) NOT NULL)",
    `INSERT INTO \`${migration.schemaName}\`.\`${migration.tableName}\` (id, tracer, payload) ` +
      `VALUES (1, ${sqlLiteral(migration.tracer)}, ${sqlLiteral(migration.payload)})`,
  ].join("; ");
  await runDocker("mysql-src-ci", [
    `mysql`,
    `-u${MYSQL_USER}`,
    `-p${MYSQL_PASS}`,
    "-e",
    sql,
  ]);
}

async function prepareTargetTable(migration: MigrationCase): Promise<void> {
  const sql = [
    `DROP SCHEMA IF EXISTS "${migration.schemaName}" CASCADE`,
    `CREATE SCHEMA "${migration.schemaName}"`,
    `CREATE TABLE "${migration.schemaName}"."${migration.tableName}" (` +
      "id INTEGER PRIMARY KEY, tracer VARCHAR(128) NOT NULL, payload VARCHAR(256) NOT NULL)",
  ].join("; ");
  await runDocker("postgres-dst-ci", [
    "psql",
    "-U",
    "postgres",
    "-d",
    "postgres",
    "-v",
    "ON_ERROR_STOP=1",
    "-c",
    sql,
  ]);
}

async function createSnapshotTask(
  auth: AuthCookies,
  migration: MigrationCase,
): Promise<string> {
  const res = await authedFetch({
    path: "/tasks",
    method: "POST",
    auth,
    body: {
      name: `e2e_mysql_pg_${Date.now().toString(36)}`,
      kind: "snapshot",
      engineSource: "mysql",
      engineTarget: "postgres",
      sourceEndpoint: { url: SOURCE_DSN },
      targetEndpoint: { url: TARGET_DSN },
      extractor: {
        extract_type: "snapshot",
        batch_size: 100,
        max_connections: 1,
      },
      sinker: { sink_type: "write", batch_size: 50, max_connections: 1 },
      filter: {
        do_dbs: migration.schemaName,
        do_tbs: `${migration.schemaName}.${migration.tableName}`,
      },
      router: {},
      parallelizer: { parallel_type: "snapshot", parallel_size: 1 },
      pipeline: { buffer_size: 100, checkpoint_interval_secs: 1 },
      resumer: { resume_type: "from_log" },
      runtime: {
        log_level: "info",
        log_dir: "./logs",
        log4rs_file: "./log4rs.yaml",
      },
      metrics: { http_host: "127.0.0.1" },
    },
  });
  const bodyText = await res.text();
  expect(res.status, `Task creation failed: ${res.status} ${bodyText}`).toBe(
    201,
  );
  const task = parseTaskResponse(bodyText);
  expect(
    typeof task.id,
    `Task creation response has no string id: ${bodyText}`,
  ).toBe("string");
  return String(task.id);
}

async function getTaskStatus(
  auth: AuthCookies,
  taskId: string,
): Promise<string> {
  const res = await authedFetch({
    path: `/tasks/${taskId}`,
    method: "GET",
    auth,
  });
  const bodyText = await res.text();
  expect(
    res.status,
    `Task status fetch failed for ${taskId}: ${res.status} ${bodyText}`,
  ).toBe(200);
  const task = parseTaskResponse(bodyText);
  return typeof task.status === "string" ? task.status : "unknown";
}

async function waitForTerminalStatus(
  auth: AuthCookies,
  taskId: string,
): Promise<string> {
  const startedAt = Date.now();
  const deadline = startedAt + 180_000;
  let lastStatus = "unknown";
  while (Date.now() < deadline) {
    lastStatus = await getTaskStatus(auth, taskId);
    if (TERMINAL_STATUSES.some((status) => status === lastStatus)) {
      return lastStatus;
    }
    await new Promise((resolve) => setTimeout(resolve, 2_000));
  }
  throw new Error(
    `Task ${taskId} did not reach a terminal status within 180000ms; last status: ${lastStatus}`,
  );
}

async function latestRun(
  auth: AuthCookies,
  taskId: string,
): Promise<RunItem | null> {
  const res = await authedFetch({
    path: `/tasks/${taskId}/runs?size=1`,
    method: "GET",
    auth,
  });
  const bodyText = await res.text();
  expect(
    res.status,
    `Latest run fetch failed for ${taskId}: ${res.status} ${bodyText}`,
  ).toBe(200);
  const body = parseRunResponse(bodyText);
  if (!Array.isArray(body.items) || body.items.length === 0) {
    return null;
  }
  const item = body.items[0];
  return isRunItem(item) ? item : null;
}

async function verifyTargetRow(migration: MigrationCase): Promise<string> {
  const sql =
    `SELECT COUNT(*) FROM "${migration.schemaName}"."${migration.tableName}" ` +
    `WHERE id = 1 AND tracer = ${sqlLiteral(migration.tracer)} AND payload = ${sqlLiteral(migration.payload)}`;
  return runDocker("postgres-dst-ci", [
    "psql",
    "-U",
    "postgres",
    "-d",
    "postgres",
    "-t",
    "-A",
    "-v",
    "ON_ERROR_STOP=1",
    "-c",
    sql,
  ]);
}

test.describe("real backend MySQL to PostgreSQL snapshot migration", () => {
  test.skip(
    !process.env.E2E_REAL_BACKEND,
    "Requires E2E_REAL_BACKEND=1 and real dt-console-server/Docker stack",
  );

  test("copies a unique MySQL source row into PostgreSQL", async () => {
    test.setTimeout(240_000);

    const migration = newMigrationCase();
    console.log(
      `MIGRATION_CASE schema=${migration.schemaName} table=${migration.tableName} tracer=${migration.tracer}`,
    );
    await seedSourceRow(migration);
    await prepareTargetTable(migration);

    const auth = await apiLogin();
    await activateLicense(auth);
    const taskId = await createSnapshotTask(auth, migration);
    const startRes = await authedFetch({
      path: `/tasks/${taskId}/start`,
      method: "POST",
      auth,
    });
    const startBody = await startRes.text();
    expect(
      [200, 202],
      `Task start failed: ${startRes.status} ${startBody}`,
    ).toContain(startRes.status);

    const finalStatus = await waitForTerminalStatus(auth, taskId);
    expect(
      SUCCESS_STATUSES,
      `Task ${taskId} ended with ${finalStatus}; latest run: ${JSON.stringify(await latestRun(auth, taskId))}`,
    ).toContain(finalStatus);

    const rowCount = await verifyTargetRow(migration);
    console.log(`POSTGRES_ROW_COUNT ${rowCount}`);
    expect(
      rowCount,
      `PostgreSQL target row missing for ${migration.schemaName}.${migration.tableName}`,
    ).toBe("1");
  });
});

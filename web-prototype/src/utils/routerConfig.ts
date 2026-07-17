type ColumnMapEntry = {
  db: string;
  tb: string;
  col_map: Record<string, string>;
};

function normalizePairLines(value?: string): string {
  return (value || "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.includes(":"))
    .map((line) => {
      const sep = line.indexOf(":");
      return `${line.slice(0, sep).trim()}:${line.slice(sep + 1).trim()}`;
    })
    .join(",");
}

function isJsonColumnMap(value: string): boolean {
  const trimmed = value.trim();
  return trimmed.startsWith("[") || trimmed.startsWith("json:[");
}

function splitQualifiedColumn(value: string): [string, string, string] {
  const parts = value.split(".");
  if (parts.length !== 3 || parts.some((part) => !part.trim())) {
    throw new Error(`Invalid column mapping endpoint: ${value}`);
  }
  return [parts[0].trim(), parts[1].trim(), parts[2].trim()];
}

function buildColumnMap(value: string): string {
  const groups = new Map<string, ColumnMapEntry>();
  for (const line of value.split("\n").map((part) => part.trim())) {
    if (!line) continue;
    const sep = line.indexOf(":");
    if (sep < 0) throw new Error(`Invalid column mapping: ${line}`);

    const [srcDb, srcTb, srcCol] = splitQualifiedColumn(line.slice(0, sep));
    const [, , dstCol] = splitQualifiedColumn(line.slice(sep + 1));
    const key = `${srcDb}.${srcTb}`;
    const entry = groups.get(key) ?? { db: srcDb, tb: srcTb, col_map: {} };
    entry.col_map[srcCol] = dstCol;
    groups.set(key, entry);
  }
  return JSON.stringify([...groups.values()]);
}

function normalizeColumnMap(value?: string): string {
  const trimmed = value?.trim() || "";
  if (!trimmed) return "";
  if (isJsonColumnMap(trimmed)) return trimmed;
  return buildColumnMap(trimmed);
}

export function buildRouterConfig(
  dbMap: string | undefined,
  tbMap: string | undefined,
  colMap: string | undefined,
  topicMap: string | undefined,
  includeTopicMap: boolean,
): Record<string, string> | undefined {
  const router: Record<string, string> = {};
  const db_map = normalizePairLines(dbMap);
  const tb_map = normalizePairLines(tbMap);
  const col_map = normalizeColumnMap(colMap);
  const topic_map = includeTopicMap ? normalizePairLines(topicMap) : "";

  if (db_map) router.db_map = db_map;
  if (tb_map) router.tb_map = tb_map;
  if (col_map) router.col_map = col_map;
  if (topic_map) router.topic_map = topic_map;
  return Object.keys(router).length > 0 ? router : undefined;
}

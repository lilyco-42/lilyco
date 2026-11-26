import { text } from "@/strings";

export type Value = string | boolean | number;
export type Fields = Record<string, Value>;
export type Row = { id: number; data: Fields };
export type Page = { items: Row[]; next: number | null };
export type Kind = "string" | "boolean" | "integer";
export type Field = { name: string; kind: Kind };
export type Collection = { name: string; path: string; fields: Field[] };
export type Operation = { method: string; path: string; id: string; body: unknown; responses: unknown };
export type Catalog = { name: string; version: string; collections: Collection[]; operations: Operation[] };

function object(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : undefined;
}

async function request(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(path, init).catch(() => {
    throw new Error(text("network_error"));
  });
  if (response.status === 204) return undefined;
  const data: unknown = await response.json().catch(() => undefined);
  if (response.ok) return data;
  const problem = object(data);
  throw new Error(typeof problem?.message === "string" ? problem.message : text("network_error"));
}

function fields(schema: unknown): Field[] | undefined {
  const properties = object(object(schema)?.properties);
  if (!properties) return undefined;
  const definitions: Field[] = [];
  for (const [name, property] of Object.entries(properties)) {
    const kind = object(property)?.type;
    if (kind !== "string" && kind !== "boolean" && kind !== "integer") return undefined;
    definitions.push({ name, kind });
  }
  return definitions;
}

function requestSchema(body: unknown): unknown {
  return object(object(object(body)?.content)?.["application/json"])?.schema;
}

export async function databaseReady(): Promise<boolean> {
  try { return (await fetch("/readyz")).status === 204; }
  catch { return false; }
}

export async function catalog(): Promise<Catalog> {
  const doc = object(await request("/openapi.json"));
  const info = object(doc?.info);
  const paths = object(doc?.paths);
  if (!paths || typeof info?.title !== "string" || typeof info.version !== "string") {
    throw new Error(text("invalid_response"));
  }

  const collections: Collection[] = [];
  const operations: Operation[] = [];
  for (const [path, methods] of Object.entries(paths)) {
    const spec = object(methods);
    if (!spec) continue;
    const name = /^\/api\/([a-z][a-z0-9-]*)$/.exec(path)?.[1];
    const params = fields(requestSchema(object(spec.post)?.requestBody));
    if (name && params?.length) collections.push({ name, path, fields: params });
    for (const [method, value] of Object.entries(spec)) {
      const op = object(value);
      if (typeof op?.operationId !== "string") continue;
      operations.push({
        method, path, id: op.operationId, body: requestSchema(op.requestBody), responses: op.responses,
      });
    }
  }
  return { name: info.title, version: info.version, collections, operations };
}

function isValue(value: unknown): value is Value {
  return typeof value === "string" || typeof value === "boolean" || Number.isSafeInteger(value);
}

function row(value: unknown): Row {
  const record = object(value);
  const data = object(record?.data);
  if (typeof record?.id !== "number" || !Number.isSafeInteger(record.id) || !data) {
    throw new Error(text("invalid_response"));
  }
  const values = Object.entries(data).flatMap(([key, entry]) => isValue(entry) ? [[key, entry] as const] : []);
  if (values.length !== Object.keys(data).length) throw new Error(text("invalid_response"));
  return { id: record.id, data: Object.fromEntries(values) };
}

export async function list(path: string, after?: number): Promise<Page> {
  const data = object(await request(after ? path + "?after=" + after : path));
  if (!Array.isArray(data?.items) || (data.next !== null && !Number.isSafeInteger(data.next))) {
    throw new Error(text("invalid_response"));
  }
  return { items: data.items.map(row), next: typeof data.next === "number" ? data.next : null };
}

export async function save(path: string, data: Fields, id?: number): Promise<Row> {
  return row(await request(id === undefined ? path : path + "/" + id, {
    method: id === undefined ? "POST" : "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  }));
}

export async function remove(path: string, id: number): Promise<void> {
  await request(path + "/" + id, { method: "DELETE" });
}

export function title(value: string): string {
  return value.split(/[-_]/).map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join(" ");
}

export async function tryOperation(operation: Operation, id: string, example: string): Promise<string> {
  const route = operation.path.replace("{id}", encodeURIComponent(id));
  // The endpoint reference only sends requests to paths under the same API.
  if (!/^\/(?:api\/[a-z][a-z0-9-]*(?:\/[1-9][0-9]*)?|healthz|readyz)$/.test(route)) {
    throw new Error(text("invalid_id"));
  }
  const input = operation.body === undefined ? undefined : JSON.stringify(JSON.parse(example) as unknown);
  const response = await fetch(route, {
    method: operation.method.toUpperCase(),
    headers: input === undefined ? undefined : { "Content-Type": "application/json" },
    body: input,
  });
  const result = await response.text();
  return String(response.status) + (result ? "\n" + result : "");
}

export function exampleFor(schema: unknown): string {
  return JSON.stringify(Object.fromEntries((fields(schema) ?? []).map(({ name, kind }) => [
    name, kind === "boolean" ? false : kind === "integer" ? 0 : "",
  ])), null, 2);
}

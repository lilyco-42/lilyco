import type { UiCatalog, UiRunRequest } from "./contract";

export function config(name: string): string {
  return document.querySelector<HTMLMetaElement>(`meta[name="lilyco-${name}"]`)?.content ?? "";
}

export async function fetchCatalog(signal: AbortSignal): Promise<UiCatalog> {
  const catalog = new URL("/ui/catalog", location.origin);
  const command = new URLSearchParams(location.search).get("cmd");
  if (command) catalog.searchParams.set("cmd", command);
  const response = await fetch(catalog, { signal });
  if (!response.ok) throw new Error(await response.text());
  const data: UiCatalog = await response.json();
  if (!Array.isArray(data.commands) || !data.commands.some(command => command.name === data.selected)) {
    throw new Error("Invalid UI catalog");
  }
  return data;
}

type Requests = {
  "/run": { body: UiRunRequest; response: { session_id: string } };
  "/upload": { body: { name: string; data_b64: string }; response: { path: string; size: number } };
  "/pick": { body: Record<string, never>; response: { path: string | null; error?: string } };
};

async function post(path: string, data: object): Promise<Response> {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-Lilyco-Token": config("token") },
    body: JSON.stringify(data),
  });
  if (!response.ok) throw new Error((await response.text()) || `HTTP ${response.status}`);
  return response;
}

export async function request<K extends keyof Requests>(
  path: K, body: Requests[K]["body"],
): Promise<Requests[K]["response"]> {
  return (await post(path, body)).json();
}

export async function cancelRun(id: string) {
  await post("/cancel/" + encodeURIComponent(id), {});
}

export interface Announcement {
  id: number;
  published_date: string;
  title: string;
  summary: string;
  link_url: string;
  pinned: boolean;
}

export interface AiModel {
  id: string;
  display_name: string;
  provider: string;
  group_name: string;
  category: string;
  input_credits_per_m: number;
  output_credits_per_m: number;
  available: boolean;
  latency_ms: number;
  note: string;
}

export interface StudioProduct {
  code: string;
  name: string;
  desc: string;
  points_price: number;
  price_cny?: number;
  price_usdt?: number;
  duration_days?: number;
  traffic_gb?: number;
  sale_open?: boolean;
  delivery_kind?: string;
}

export interface DownloadItem {
  slug: string;
  name: string;
  repo: string;
  category: string;
  stars: number;
  version: string;
  platform: string;
  summary: string;
  asset_name: string;
  asset_size_bytes?: number;
  quark_url: string;
  github_release_url: string;
  github_download_url?: string;
  download_count: number;
}

export interface MirrorLink {
  id: string;
  label: string;
  url: string;
  recommended: boolean;
}

export interface ResolvedAsset {
  name: string;
  os: "win" | "mac" | "linux" | "android" | "ios" | "other" | string;
  arch: "x64" | "arm64" | "x86" | "universal" | string;
  ext: string;
  size_bytes: number;
  is_auxiliary: boolean;
  mirrors: MirrorLink[];
}

export interface ReleaseAssetsResponse {
  slug: string;
  name: string;
  repo: string;
  version: string;
  release_url: string;
  quark_url: string;
  source: "github_live" | "catalog_fallback" | string;
  assets: ResolvedAsset[];
}

export interface ProxyQualityNode {
  name: string;
  region: string;
  protocol: string;
  state: "reachable" | "unreachable" | "unverified" | "unknown";
  latency_ms: number | null;
  jitter_ms: number | null;
}

export interface ProxyQualityReport {
  source: string;
  tested_at: string | null;
  measurement: string | null;
  stale: boolean;
  nodes: ProxyQualityNode[];
}

export interface OfficialProxyAudit {
  checked_unix: number;
  rows: {
    region: string;
    protocol: string;
    state: "approved" | "https_failed" | "slow";
    elapsed_ms: number | null;
  }[];
}

export interface UserPoints {
  balance: number;
  paid_balance: number;
  bonus_balance: number;
}

export interface UserEntitlementMeta {
  sub_url?: string;
  traffic_gb?: number;
  expires_at?: string;
  panel_username?: string;
  purchase_product?: string;
}

export interface UserEntitlement {
  product: string;
  license_key: string;
  status: string;
  expires_at?: string;
  created_at?: number;
  meta?: UserEntitlementMeta;
}

export interface MeResponse {
  ok: boolean;
  is_admin?: boolean;
  username: string;
  email: string;
  points: UserPoints;
  entitlements: UserEntitlement[];
  orders?: OrderRecord[];
}

export interface OrderRecord {
  order_no: string;
  product: string;
  status: string;
  payment_method?: string;
  points_delta?: number;
  points_amount?: number;
  created_at: string | number;
}

const TOKEN_KEY = import.meta.env.VITE_TOKEN_KEY || "lain42_studio_token";
const ADMIN_TOKEN_KEY = TOKEN_KEY + "_admin";

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) || "";
}

export function getPoolApiBase(): string {
  return import.meta.env.VITE_POOL_API_BASE || "/api/pool/api/v1";
}

export function poolApiPath(path: string): string {
  const base = getPoolApiBase().replace(/\/+$/, "");
  const suffix = path.startsWith("/") ? path : `/${path}`;
  return `${base}${suffix}`;
}

export function setToken(token: string, adminToken?: string): void {
  if (token) localStorage.setItem(TOKEN_KEY, token);
  else localStorage.removeItem(TOKEN_KEY);
  if (token && adminToken) sessionStorage.setItem(ADMIN_TOKEN_KEY, adminToken);
  else sessionStorage.removeItem(ADMIN_TOKEN_KEY);
}

export async function apiFetch<T>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const headers: Record<string, string> = {
    ...(options.body ? { "Content-Type": "application/json" } : {}),
    ...(options.headers as Record<string, string>),
  };
  const tok = getToken();
  if (tok && !headers["Authorization"]) {
    headers["Authorization"] = `Bearer ${tok}`;
  }
  if (path.startsWith("/api/admin/") || path === "/studio/api/me") {
    const adminToken = sessionStorage.getItem(ADMIN_TOKEN_KEY);
    if (adminToken) headers["X-Admin-Session"] = adminToken;
  }
  const resp = await fetch(path, { ...options, headers });
  const data = (await resp.json().catch(() => ({}))) as T & {
    detail?: string;
    error?: string | { message?: string; code?: string };
  };
  if (!resp.ok) {
    const message =
      (typeof data.error === "string" ? data.error : data.error?.message) ||
      data.detail ||
      (resp.status === 403 && path.startsWith("/api/admin/") ? "管理员会话无效或已过期，请重新登录" : undefined) ||
      `请求失败 (${resp.status})`;
    throw new Error(message);
  }
  return data;
}

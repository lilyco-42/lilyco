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
  quark_url: string;
  github_release_url: string;
  download_count: number;
}

export interface ProxyQualityNode {
  name: string;
  region: string;
  protocol: string;
  kbps: number;
  peak_kbps: number;
  status: string;
}

export interface UserPoints {
  balance: number;
  paid_balance: number;
  bonus_balance: number;
}

export interface UserEntitlement {
  product: string;
  license_key: string;
  status: string;
  expires_at: string;
}

export interface MeResponse {
  ok: boolean;
  username: string;
  email: string;
  points: UserPoints;
  entitlements: UserEntitlement[];
}

export interface OrderRecord {
  order_no: string;
  product: string;
  status: string;
  payment_method: string;
  points_delta: number;
  created_at: string;
}

const TOKEN_KEY = "lain42_studio_token";

export function getToken(): string {
  return localStorage.getItem(TOKEN_KEY) || "";
}

export function setToken(token: string): void {
  if (token) localStorage.setItem(TOKEN_KEY, token);
  else localStorage.removeItem(TOKEN_KEY);
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
  const resp = await fetch(path, { ...options, headers });
  const data = (await resp.json().catch(() => ({}))) as T & {
    detail?: string;
    error?: string;
  };
  if (!resp.ok) {
    throw new Error(data.detail || data.error || `请求失败 (${resp.status})`);
  }
  return data;
}

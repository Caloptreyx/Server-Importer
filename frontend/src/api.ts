import { axiosInstance } from '@/api/axios.ts';

export const PACKAGE = 'dev.caloptreyx.serverimporter';
export const ADMIN_BASE = `/api/admin/extensions/${PACKAGE}`;
const base = (server: string) => `/api/client/servers/${server}/server-importer`;

export type AuthPayload =
  | { type: 'password'; password: string }
  | { type: 'private_key'; private_key: string; passphrase?: string };

export type AuthMethod = 'password' | 'private_key';

export type EntryKind = 'directory' | 'file' | 'symlink' | 'other';

export interface RemoteEntry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number;
  modified_at: number | null;
  permissions: number | null;
}

export interface BrowseResponse {
  host_fingerprint: string;
  resolved_path: string;
  entries: RemoteEntry[];
}

export interface PreviewResponse {
  host_fingerprint: string;
  resolved_path: string;
  file_count: number;
  directory_count: number;
  symlink_count: number;
  total_bytes: number;
  truncated: boolean;
  truncation_reason: string | null;
}

export type ImportPhase = 'connecting' | 'scanning' | 'transferring';

export interface ActiveImport {
  id: string;
  host: string;
  port: number;
  username: string;
  auth_method: string;
  host_fingerprint: string | null;
  source_path: string;
  destination_path: string;
  phase: ImportPhase;
  files_total: number;
  files_transferred: number;
  files_skipped: number;
  files_failed: number;
  bytes_total: number;
  bytes_transferred: number;
  current_file: string | null;
  bytes_per_second: number;
  eta_seconds: number | null;
  started_at: string;
  recent_errors: string[];
}

export type JobStatus = 'running' | 'completed' | 'failed' | 'cancelled';

export interface ImportJobRecord {
  id: string;
  server_uuid: string;
  user_uuid: string | null;
  user_username: string | null;
  host: string;
  port: number;
  username: string;
  auth_method: string;
  host_fingerprint: string | null;
  source_path: string;
  destination_path: string;
  status: JobStatus;
  files_total: number;
  files_transferred: number;
  files_skipped: number;
  files_failed: number;
  bytes_total: number;
  bytes_transferred: number;
  error: string | null;
  started_at: string;
  finished_at: string | null;
}

export interface SavedLogin {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_method: AuthMethod;
  has_passphrase: boolean;
  default_source_path: string;
  host_fingerprint: string | null;
  created_at: string;
  updated_at: string;
  last_used_at: string | null;
}

export interface ImporterLimits {
  max_import_size_mb: number;
  max_import_files: number;
  max_concurrent_imports: number;
}

export interface StatusResponse {
  active: ActiveImport | null;
  history: ImportJobRecord[];
  saved_logins: SavedLogin[];
  allow_saved_logins: boolean;
  limits: ImporterLimits;
}

export interface BrowseRequest {
  saved_login_id?: string;
  host: string;
  port: number;
  username: string;
  auth?: AuthPayload;
  path: string;
  expected_fingerprint?: string;
}

export interface StartImportRequest {
  saved_login_id?: string;
  host: string;
  port: number;
  username: string;
  auth?: AuthPayload;
  source_path: string;
  destination_path: string;
  expected_fingerprint?: string;
}

export interface CreateSavedLoginRequest {
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthPayload;
  default_source_path: string;
  host_fingerprint?: string;
}

export interface UpdateSavedLoginRequest {
  name: string;
  host: string;
  port: number;
  username: string;
  auth?: AuthPayload;
  default_source_path: string;
  host_fingerprint?: string;
}

export interface Settings {
  max_concurrent_imports: number;
  max_import_size_mb: number;
  max_import_files: number;
  connection_timeout_secs: number;
  allow_private_networks: boolean;
  allow_saved_logins: boolean;
}

export async function getStatus(server: string): Promise<StatusResponse> {
  const { data } = await axiosInstance.get<StatusResponse>(`${base(server)}/status`);
  return data;
}

export async function createSavedLogin(server: string, payload: CreateSavedLoginRequest): Promise<SavedLogin> {
  const { data } = await axiosInstance.post<{ login: SavedLogin }>(`${base(server)}/saved-logins`, payload);
  return data.login;
}

export async function updateSavedLogin(
  server: string,
  loginId: string,
  payload: UpdateSavedLoginRequest,
): Promise<SavedLogin> {
  const { data } = await axiosInstance.put<{ login: SavedLogin }>(`${base(server)}/saved-logins/${loginId}`, payload);
  return data.login;
}

export async function deleteSavedLogin(server: string, loginId: string): Promise<{ deleted: boolean }> {
  const { data } = await axiosInstance.delete<{ deleted: boolean }>(`${base(server)}/saved-logins/${loginId}`);
  return data;
}

export async function browseRemote(server: string, payload: BrowseRequest): Promise<BrowseResponse> {
  const { data } = await axiosInstance.post<BrowseResponse>(`${base(server)}/browse`, payload);
  return data;
}

export async function previewRemote(server: string, payload: BrowseRequest): Promise<PreviewResponse> {
  const { data } = await axiosInstance.post<PreviewResponse>(`${base(server)}/preview`, payload);
  return data;
}

export async function startImport(server: string, payload: StartImportRequest): Promise<{ active: ActiveImport }> {
  const { data } = await axiosInstance.post<{ active: ActiveImport }>(`${base(server)}/start`, payload);
  return data;
}

export async function cancelImport(server: string): Promise<{ cancelled: boolean; job_id: string }> {
  const { data } = await axiosInstance.post<{ cancelled: boolean; job_id: string }>(`${base(server)}/cancel`);
  return data;
}

export async function getSettings(): Promise<Settings> {
  const { data } = await axiosInstance.get<{ settings: Settings }>(`${ADMIN_BASE}/settings`);
  return data.settings;
}

export async function updateSettings(settings: Settings): Promise<Settings> {
  const { data } = await axiosInstance.put<{ settings: Settings }>(`${ADMIN_BASE}/settings`, settings);
  return data.settings;
}

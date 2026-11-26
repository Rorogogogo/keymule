// What keymule's local API sends. Only names and metadata: no secrets.

export type Kind = 'http' | 'password' | 'private-key' | 'aws' | 'ssh' | 'login'

export interface Credential {
  name: string
  service: string
  url: string
  description: string
  auth: string
  hosts?: string[]
  kind?: Kind
  ssh?: { label: string; auth: string } | null
  installed?: string[]
  missing_secret?: boolean
  file_edited?: boolean
}

export interface ServerSecret {
  name: string
  description: string
  env?: string[]
  headers?: string[]
  arg_secrets?: number
  used_by: string[]
}

export interface List {
  credentials: Credential[]
  servers: ServerSecret[]
  vault_problem?: string
}

export interface AuditLine {
  ts: number
  name: string
  method: string
  url: string
  status: number
}

export interface Update {
  current: string
  latest?: string
  notes?: string
  command?: string
  replaced?: boolean
}

export interface StateReply {
  list: List | null
  audit: AuditLine[]
  version: string
  update?: Update | null
  host?: string
  error?: string
}

export interface CommandReply {
  ok: boolean
  stdout?: string
  stderr?: string
}

export interface Plan {
  text: string
  has: boolean
  names: string[]
  summary: string
}

export interface Severity { severity: 'leaked' | 'plaintext' | 'readable'; place: string; date?: string }
export interface LeakEntry { masked: string; label: string; stored_as?: string; locations: Severity[] }
export interface Report {
  entries: LeakEntry[]
  stats?: { transcripts?: number; configs?: string[]; env_files?: number }
}
export type Doctor = null | 'running' | { error: string } | Report

export interface ScanStatus {
  id?: number
  running?: boolean
  error?: string
  report?: Report
  phase?: string
  done: number
  total: number
  elapsed_ms: number
  lines: number
  found: number
  log: string[]
}

export const USE_KINDS = ['api', 'command', 'mcp', 'reveal'] as const
export type UseKind = (typeof USE_KINDS)[number]
export type Day = { d: number; total: number; failed: number } & Record<UseKind, number>
export type KeyUse = { name: string; total: number; failed: number; last: number; spark: number[] } & Record<UseKind, number>
export interface Stats { days: number; today: number; daily: Day[]; keys: KeyUse[] }

// ---- questions asked in the page ----

export interface AskSpec {
  title: string
  body?: string
  icon?: string
  tone?: string
  buttons: string[]
  default: string
  cancel: string
  primary?: string
  destructive?: string
  secure?: boolean
  multiline?: boolean
  reveal?: boolean
}
export interface AskRequest { id: string; epk: string; spec: AskSpec }
export interface Pending { raw: string; req: AskRequest }
export interface Device { id: string; name: string }
export interface Show { id: string; iv: string; ct: string }
export interface Revealed { name: string; items?: [string, string][]; copy?: string | null }

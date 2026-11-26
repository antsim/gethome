/**
 * Types shared with the Rust backend. Keep in sync with the serde models in
 * `src-tauri/src` (field names are camelCase on the wire).
 */

export type TimerStatus = 'NotStarted' | 'Working' | 'OnBreak' | 'Done';

export type SyncWarning = 'none' | 'rateLimited' | 'offline' | 'authError' | 'noToken';

/** Payload of the `timer-state` event (`engine/compute.rs`). */
export interface TimerViewModel {
  status: TimerStatus;
  /** Negative means overtime. */
  remainingSecs: number;
  workedSecs: number;
  targetSecs: number;
  breakSecs: number;
  /** ISO timestamp of the estimated finish, only while working. */
  eta: string | null;
  running: boolean;
  lastSynced: string | null;
  syncWarning: SyncWarning;
}

/** `settings/mod.rs` → `Settings`. */
export interface Settings {
  /** Minutes per weekday, Monday first (7 items). */
  dailyTargets: number[];
  /** IANA time zone, or `null` to follow the system. */
  timezone: string | null;
  showSeconds: boolean;
  autostart: boolean;
  /** Percent, 60–100. */
  windowOpacity: number;
  timerWindowVisible: boolean;
  /** Open the day summary automatically when the target is reached. */
  autoShowSummary: boolean;
}

/** `engine/summary.rs` → `SummaryTask`: entries with the same description. */
export interface SummaryTask {
  /** Trimmed Toggl description; empty when the entries have none. */
  description: string;
  workedSecs: number;
  /** Rounded time to book, a positive multiple of 30. */
  bookedMinutes: number;
}

/** `engine/summary.rs` → `DaySummary`. */
export interface DaySummary {
  /** Local date, `YYYY-MM-DD`. */
  date: string;
  workedSecs: number;
  bookedMinutes: number;
  tasks: SummaryTask[];
  running: boolean;
}

/** Which Toggl product the API token belongs to (detected from the token). */
export type TogglFlavor = 'track' | 'focus';

/** `toggl/models.rs` → `Account`. */
export interface AccountInfo {
  flavor: TogglFlavor;
  /** Not available for Toggl 2.0. */
  name: string | null;
  workspaceId: number;
  workspaceName: string | null;
  /** Toggl 2.0 only. */
  organizationId: number | null;
}

export interface SettingsView {
  settings: Settings;
  account: AccountInfo | null;
  hasToken: boolean;
  systemTimezone: string;
}

export type RefreshOutcome = 'refreshed' | 'throttled' | 'rateLimited' | 'failed';

/** Error returned by commands (`error.rs`). `message` is user-facing. */
export interface AppError {
  code: string;
  message: string;
}

export const TauriEvent = {
  TimerState: 'timer-state',
  SettingsChanged: 'settings-changed',
  SummaryVisibility: 'summary-visibility',
} as const;

/** Daily targets are whole or half hours (`settings/mod.rs`). */
export const TARGET_STEP_MINUTES = 30;

export const MIN_OPACITY = 60;
export const MAX_OPACITY = 100;

import { Injectable } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  AccountInfo,
  AppError,
  DaySummary,
  RefreshOutcome,
  Settings,
  SettingsView,
  TimerViewModel,
} from './models';

/**
 * Typed wrappers around the Rust commands (`src-tauri/src/commands.rs`) and
 * events. Components never call `invoke` directly, which keeps them easy to
 * test with a mock of this service.
 */
@Injectable({ providedIn: 'root' })
export class TauriCommands {
  getState(): Promise<TimerViewModel> {
    return invoke('get_state');
  }

  refreshNow(): Promise<RefreshOutcome> {
    return invoke('refresh_now');
  }

  stopTimer(): Promise<void> {
    return invoke('stop_timer');
  }

  resumeTimer(): Promise<void> {
    return invoke('resume_timer');
  }

  getSettings(): Promise<SettingsView> {
    return invoke('get_settings');
  }

  saveSettings(settings: Settings): Promise<Settings> {
    return invoke('save_settings', { settings });
  }

  setApiToken(token: string): Promise<AccountInfo> {
    return invoke('set_api_token', { token });
  }

  clearApiToken(): Promise<void> {
    return invoke('clear_api_token');
  }

  testConnection(): Promise<AccountInfo> {
    return invoke('test_connection');
  }

  toggleWindow(): Promise<void> {
    return invoke('toggle_window');
  }

  openSettings(): Promise<void> {
    return invoke('open_settings');
  }

  listTimezones(): Promise<string[]> {
    return invoke('list_timezones');
  }

  getDaySummary(): Promise<DaySummary> {
    return invoke('get_day_summary');
  }

  isSummaryOpen(): Promise<boolean> {
    return invoke('is_summary_open');
  }

  showSummary(): Promise<void> {
    return invoke('show_summary');
  }

  closeSummary(): Promise<void> {
    return invoke('close_summary');
  }

  /** Stops the timer (if running) and opens the day summary. */
  finishDay(): Promise<void> {
    return invoke('finish_day');
  }

  copyToClipboard(text: string): Promise<void> {
    return invoke('copy_to_clipboard', { text });
  }

  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
    return listen<T>(event, (e) => handler(e.payload));
  }
}

/** Normalizes a rejected command into an {@link AppError}. */
export function toAppError(error: unknown): AppError {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { code, message } = error as Partial<AppError>;
    return { code: code ?? 'unknown', message: String(message) };
  }
  return { code: 'unknown', message: typeof error === 'string' ? error : 'Tuntematon virhe.' };
}

import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';

import { SettingsService } from '../core/settings.service';
import { SummaryStateService } from '../core/summary-state.service';
import { TauriCommands, toAppError } from '../core/tauri-commands';
import { TimerStateService } from '../core/timer-state.service';
import { DaySummaryPanel } from '../summary/day-summary';
import { toTimerView } from './timer-view';

const ERROR_VISIBLE_MS = 5000;

/**
 * The small always-on-top window showing the remaining working time. The
 * backend expands it to show the day summary instead.
 */
@Component({
  selector: 'app-timer-window',
  imports: [DaySummaryPanel],
  templateUrl: './timer-window.html',
  styleUrl: './timer-window.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '[style.--surface-alpha]': 'surfaceAlpha()',
  },
})
export class TimerWindow {
  private readonly commands = inject(TauriCommands);
  private readonly timerState = inject(TimerStateService);
  private readonly settings = inject(SettingsService);
  protected readonly summaryOpen = inject(SummaryStateService).open;

  protected readonly view = computed(() => {
    const vm = this.timerState.current();
    return vm ? toTimerView(vm) : null;
  });
  protected readonly surfaceAlpha = computed(
    () => (this.settings.current()?.windowOpacity ?? 100) / 100,
  );
  protected readonly pending = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly Math = Math;
  private errorTimer: ReturnType<typeof setTimeout> | undefined;

  protected async toggleTimer(): Promise<void> {
    const view = this.view();
    if (!view || this.pending()) {
      return;
    }
    this.pending.set(true);
    try {
      await (view.running ? this.commands.stopTimer() : this.commands.resumeTimer());
      this.showError(null);
    } catch (err) {
      this.showError(toAppError(err).message);
    } finally {
      this.pending.set(false);
    }
  }

  protected async finishDay(): Promise<void> {
    if (this.pending()) {
      return;
    }
    this.pending.set(true);
    try {
      await this.commands.finishDay();
      this.showError(null);
    } catch (err) {
      this.showError(toAppError(err).message);
    } finally {
      this.pending.set(false);
    }
  }

  protected showSummary(): void {
    void this.commands.showSummary();
  }

  protected openSettings(): void {
    void this.commands.openSettings();
  }

  private showError(message: string | null): void {
    clearTimeout(this.errorTimer);
    this.error.set(message);
    if (message) {
      this.errorTimer = setTimeout(() => this.error.set(null), ERROR_VISIBLE_MS);
    }
  }
}

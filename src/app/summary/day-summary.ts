import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';

import type { DaySummary } from '../core/models';
import { SummaryStateService } from '../core/summary-state.service';
import { TauriCommands, toAppError } from '../core/tauri-commands';
import { TimerStateService } from '../core/timer-state.service';
import { toSummaryView, type SummaryRow } from './summary-view';

const FLASH_MS = 1200;
const ERROR_VISIBLE_MS = 5000;

/**
 * The day's tasks rounded to half hours, shown in the expanded floating
 * window. Clicking a row copies `hours<TAB>description` for the timesheet.
 */
@Component({
  selector: 'app-day-summary',
  templateUrl: './day-summary.html',
  styleUrl: './day-summary.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '(document:keydown.escape)': 'close()',
  },
})
export class DaySummaryPanel {
  private readonly commands = inject(TauriCommands);
  private readonly timerState = inject(TimerStateService);
  private readonly summaryState = inject(SummaryStateService);

  private readonly summary = signal<DaySummary | null>(null);
  protected readonly view = computed(() => {
    const summary = this.summary();
    return summary ? toSummaryView(summary) : null;
  });
  protected readonly copied = this.summaryState.copied;
  /** Key of the row that was just copied, or `'all'`. */
  protected readonly flash = signal<string | null>(null);
  protected readonly announcement = signal('');
  protected readonly error = signal<string | null>(null);
  protected readonly pending = signal(false);
  private flashTimer: ReturnType<typeof setTimeout> | undefined;
  private errorTimer: ReturnType<typeof setTimeout> | undefined;
  private loadSeq = 0;

  constructor() {
    // Reload on every timer-state update (once per second): the running
    // entry and new syncs change the numbers.
    effect(() => {
      this.timerState.current();
      void this.load();
    });
  }

  protected async copy(row: SummaryRow): Promise<void> {
    if (await this.copyText(row.copyText)) {
      this.summaryState.markCopied(row.key);
      this.showFlash(row.key, `Kopioitu: ${row.hours} h ${row.title}`);
    }
  }

  protected async copyAll(): Promise<void> {
    const view = this.view();
    if (view && view.rows.length > 0 && (await this.copyText(view.copyAllText))) {
      this.summaryState.markCopied(...view.rows.map((row) => row.key));
      this.showFlash('all', 'Kaikki rivit kopioitu');
    }
  }

  protected async stopTimer(): Promise<void> {
    this.pending.set(true);
    try {
      await this.commands.stopTimer();
    } catch (err) {
      this.showError(toAppError(err).message);
    } finally {
      this.pending.set(false);
    }
  }

  protected close(): void {
    void this.commands.closeSummary();
  }

  private async load(): Promise<void> {
    const seq = ++this.loadSeq;
    try {
      const summary = await this.commands.getDaySummary();
      if (seq === this.loadSeq) {
        this.summary.set(summary);
      }
    } catch (err) {
      this.showError(toAppError(err).message);
    }
  }

  private async copyText(text: string): Promise<boolean> {
    try {
      await this.commands.copyToClipboard(text);
      return true;
    } catch (err) {
      this.showError(toAppError(err).message);
      return false;
    }
  }

  private showFlash(key: string, announcement: string): void {
    clearTimeout(this.flashTimer);
    this.flash.set(key);
    this.announcement.set(announcement);
    this.flashTimer = setTimeout(() => this.flash.set(null), FLASH_MS);
  }

  private showError(message: string): void {
    clearTimeout(this.errorTimer);
    this.error.set(message);
    this.errorTimer = setTimeout(() => this.error.set(null), ERROR_VISIBLE_MS);
  }
}

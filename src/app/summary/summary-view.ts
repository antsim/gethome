import { formatDuration } from '../core/format';
import type { DaySummary, SummaryTask } from '../core/models';

const WEEKDAYS_SHORT = ['su', 'ma', 'ti', 'ke', 'to', 'pe', 'la'] as const;
export const NO_DESCRIPTION = '(ei kuvausta)';

/** One row of the summary list. */
export interface SummaryRow {
  /** Identifies the row and the exact values that were copied. */
  key: string;
  /** Booked hours, e.g. `4,0`. */
  hours: string;
  title: string;
  hasDescription: boolean;
  /** Actual tracked time, e.g. `3:41`. */
  worked: string;
  /** What a click copies: hours, a tab and the Toggl description. */
  copyText: string;
}

/** What the summary panel renders, derived from the backend summary. */
export interface SummaryView {
  /** E.g. `to 26.11.` */
  dateLabel: string;
  /** E.g. `Togglessa 7:32 → kirjataan 7,5 h` */
  totals: string;
  total: string;
  rows: SummaryRow[];
  /** All rows, one per line. */
  copyAllText: string;
  running: boolean;
}

/** Minutes as decimal hours with a comma, e.g. 270 → `4,5`. */
export function formatHours(minutes: number): string {
  return (minutes / 60).toFixed(1).replace('.', ',');
}

/** The tab-separated line pasted into the timesheet. */
export function copyLine(task: SummaryTask): string {
  return `${formatHours(task.bookedMinutes)}\t${task.description}`;
}

export function toSummaryView(summary: DaySummary): SummaryView {
  const rows = summary.tasks.map((task) => ({
    key: `${summary.date}|${task.description}|${task.bookedMinutes}`,
    hours: formatHours(task.bookedMinutes),
    title: task.description || NO_DESCRIPTION,
    hasDescription: task.description !== '',
    worked: formatDuration(task.workedSecs, false),
    copyText: copyLine(task),
  }));
  const total = `${formatHours(summary.bookedMinutes)} h`;
  return {
    dateLabel: formatDateLabel(summary.date),
    totals: `Togglessa ${formatDuration(summary.workedSecs, false)} → kirjataan ${total}`,
    total,
    rows,
    copyAllText: rows.map((row) => row.copyText).join('\n'),
    running: summary.running,
  };
}

/** `2026-11-26` → `to 26.11.` */
export function formatDateLabel(date: string): string {
  const [year, month, day] = date.split('-').map(Number);
  const weekday = new Date(year, month - 1, day).getDay();
  return `${WEEKDAYS_SHORT[weekday]} ${day}.${month}.`;
}

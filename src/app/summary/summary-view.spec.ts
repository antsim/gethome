import type { DaySummary } from '../core/models';
import { copyLine, formatDateLabel, formatHours, toSummaryView } from './summary-view';

const summary: DaySummary = {
  date: '2026-11-26',
  workedSecs: 7 * 3600 + 32 * 60,
  bookedMinutes: 450,
  running: false,
  tasks: [
    { description: 'Koodaus: GetHome', workedSecs: 3 * 3600 + 41 * 60, bookedMinutes: 240 },
    { description: '', workedSecs: 2 * 3600 + 10 * 60, bookedMinutes: 210 },
  ],
};

describe('formatHours', () => {
  it('uses a decimal comma and one decimal', () => {
    expect(formatHours(30)).toBe('0,5');
    expect(formatHours(240)).toBe('4,0');
    expect(formatHours(450)).toBe('7,5');
  });
});

describe('copyLine', () => {
  it('copies hours, a tab and the description', () => {
    expect(copyLine(summary.tasks[0])).toBe('4,0\tKoodaus: GetHome');
  });
});

describe('toSummaryView', () => {
  it('shows booked and tracked time for each task', () => {
    const view = toSummaryView(summary);
    expect(view.dateLabel).toBe('to 26.11.');
    expect(view.totals).toBe('Togglessa 7:32 → kirjataan 7,5 h');
    expect(view.rows[0]).toMatchObject({ hours: '4,0', title: 'Koodaus: GetHome', worked: '3:41' });
  });

  it('labels a task without description but copies it empty', () => {
    const row = toSummaryView(summary).rows[1];
    expect(row.title).toBe('(ei kuvausta)');
    expect(row.hasDescription).toBe(false);
    expect(row.copyText).toBe('3,5\t');
  });

  it('copies all rows on separate lines', () => {
    expect(toSummaryView(summary).copyAllText).toBe('4,0\tKoodaus: GetHome\n3,5\t');
  });

  it('changes the row key when the booked time changes', () => {
    const before = toSummaryView(summary).rows[0].key;
    const after = toSummaryView({
      ...summary,
      tasks: [{ ...summary.tasks[0], bookedMinutes: 270 }],
    }).rows[0].key;
    expect(after).not.toBe(before);
  });
});

describe('formatDateLabel', () => {
  it('formats a Finnish short date with the weekday', () => {
    expect(formatDateLabel('2026-01-05')).toBe('ma 5.1.');
  });
});

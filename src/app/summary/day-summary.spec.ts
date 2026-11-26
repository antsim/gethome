import { TestBed } from '@angular/core/testing';

import type { DaySummary } from '../core/models';
import { TauriCommands } from '../core/tauri-commands';
import { DaySummaryPanel } from './day-summary';

describe('DaySummaryPanel', () => {
  const summary: DaySummary = {
    date: '2026-11-26',
    workedSecs: 3 * 3600,
    bookedMinutes: 180,
    running: false,
    tasks: [
      { description: 'Koodaus', workedSecs: 2 * 3600, bookedMinutes: 120 },
      { description: 'Palaveri', workedSecs: 3600, bookedMinutes: 60 },
    ],
  };
  let copyToClipboard: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    copyToClipboard = vi.fn().mockResolvedValue(undefined);
    await TestBed.configureTestingModule({
      imports: [DaySummaryPanel],
      providers: [
        {
          provide: TauriCommands,
          useValue: { getDaySummary: vi.fn().mockResolvedValue(summary), copyToClipboard },
        },
      ],
    }).compileComponents();
  });

  async function render(): Promise<HTMLElement> {
    const fixture = TestBed.createComponent(DaySummaryPanel);
    await fixture.whenStable();
    return fixture.nativeElement as HTMLElement;
  }

  it('lists tasks with booked hours', async () => {
    const element = await render();
    const rows = [...element.querySelectorAll('.task')].map((row) => row.textContent ?? '');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain('2,0');
    expect(rows[0]).toContain('Koodaus');
    expect(element.textContent).toContain('Yhteensä 3,0 h');
  });

  it('copies a row as hours, tab and description and marks it copied', async () => {
    const element = await render();
    const row = element.querySelectorAll<HTMLButtonElement>('.task')[1];
    row.click();
    await new Promise((resolve) => setTimeout(resolve));
    TestBed.tick();

    expect(copyToClipboard).toHaveBeenCalledWith('1,0\tPalaveri');
    expect(row.classList).toContain('copied');
  });

  it('copies all rows at once', async () => {
    const element = await render();
    element.querySelector<HTMLButtonElement>('.copy-all')!.click();
    await new Promise((resolve) => setTimeout(resolve));
    TestBed.tick();

    expect(copyToClipboard).toHaveBeenCalledWith('2,0\tKoodaus\n1,0\tPalaveri');
    expect(element.querySelectorAll('.task.copied')).toHaveLength(2);
  });
});

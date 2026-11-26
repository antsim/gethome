import type { TimerViewModel } from '../core/models';
import { toTimerView } from './timer-view';

function vm(overrides: Partial<TimerViewModel>): TimerViewModel {
  return {
    status: 'Working',
    remainingSecs: 3 * 3600 + 42 * 60 + 15,
    workedSecs: 3 * 3600 + 47 * 60 + 45,
    targetSecs: 27000,
    breakSecs: 0,
    eta: null,
    running: true,
    lastSynced: null,
    syncWarning: 'none',
    ...overrides,
  };
}

describe('toTimerView', () => {
  it('shows remaining time while working', () => {
    const view = toTimerView(vm({}));
    expect(view.time).toBe('3:42:15');
    expect(view.label).toBe('Työssä');
    expect(view.tone).toBe('working');
    expect(view.progress).toBeCloseTo(0.505, 2);
  });

  it('shows the break prominently', () => {
    const view = toTimerView(vm({ status: 'OnBreak', running: false, breakSecs: 720 }));
    expect(view.label).toBe('Tauolla');
    expect(view.detail).toBe('tauko 0:12');
    expect(view.tone).toBe('break');
    expect(view.canFinish).toBe(true);
  });

  it('shows overtime with a plus sign', () => {
    const view = toTimerView(vm({ status: 'Done', remainingSecs: -720, workedSecs: 27720 }));
    expect(view.time).toBe('+0:12:00');
    expect(view.detail).toBe('+0:12');
    expect(view.progress).toBe(1);
  });

  it('disables control without a valid token', () => {
    const view = toTimerView(vm({ status: 'NotStarted', running: false, syncWarning: 'noToken' }));
    expect(view.canControl).toBe(false);
    expect(view.canFinish).toBe(false);
    expect(view.warning).toContain('API -token');
  });
});

import { formatClock, formatDuration } from '../core/format';
import type { TimerViewModel } from '../core/models';

export type TimerTone = 'working' | 'break' | 'idle' | 'overtime';

/** What the floating window renders, derived from the backend view model. */
export interface TimerView {
  time: string;
  label: string;
  detail: string | null;
  tone: TimerTone;
  /** Share of the target done, 0–1. */
  progress: number;
  warning: string | null;
  running: boolean;
  /** Whether stop/resume is possible (requires a working token). */
  canControl: boolean;
  /** Whether "finish the day" makes sense: there is work today. */
  canFinish: boolean;
}

export function toTimerView(vm: TimerViewModel): TimerView {
  const overtime = vm.remainingSecs < 0;
  const time = (overtime ? '+' : '') + formatDuration(vm.remainingSecs);
  const progress =
    vm.targetSecs > 0 ? Math.min(1, vm.workedSecs / vm.targetSecs) : vm.workedSecs > 0 ? 1 : 0;
  const canControl = vm.syncWarning !== 'noToken' && vm.syncWarning !== 'authError';

  const canFinish = vm.status !== 'NotStarted' && canControl;
  const base = {
    time,
    progress,
    warning: warningText(vm),
    running: vm.running,
    canControl,
    canFinish,
  };
  switch (vm.status) {
    case 'Working':
      return {
        ...base,
        label: 'Työssä',
        detail: vm.eta ? `valmis klo ${formatClock(vm.eta)}` : null,
        tone: 'working',
      };
    case 'OnBreak':
      return {
        ...base,
        label: 'Tauolla',
        detail: `tauko ${formatDuration(vm.breakSecs, false)}`,
        tone: 'break',
      };
    case 'Done':
      return {
        ...base,
        label: 'Tavoite täynnä',
        detail: `+${formatDuration(vm.remainingSecs, false)}`,
        tone: 'overtime',
      };
    case 'NotStarted':
      return { ...base, label: 'Ei aloitettu', detail: null, tone: 'idle' };
  }
}

function warningText(vm: TimerViewModel): string | null {
  switch (vm.syncWarning) {
    case 'none':
      return null;
    case 'rateLimited':
      return 'Toggl-yhteys rajoitettu';
    case 'offline':
      return vm.lastSynced
        ? `Offline – viimeksi päivitetty klo ${formatClock(vm.lastSynced)}`
        : 'Offline';
    case 'authError':
      return 'API-token hylätty – tarkista asetukset';
    case 'noToken':
      return 'Aseta Toggl API -token asetuksissa';
  }
}

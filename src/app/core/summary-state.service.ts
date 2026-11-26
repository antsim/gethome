import { DestroyRef, Injectable, inject, signal } from '@angular/core';

import { TauriCommands } from './tauri-commands';
import { isTauri } from './tauri-env';
import { TauriEvent } from './models';

/**
 * Whether the floating window shows the day summary (owned by the backend,
 * which also resizes the window), plus the rows copied so far.
 */
@Injectable({ providedIn: 'root' })
export class SummaryStateService {
  private readonly commands = inject(TauriCommands);
  private readonly openState = signal(false);
  private readonly copiedState = signal<ReadonlySet<string>>(new Set());

  readonly open = this.openState.asReadonly();
  /** Keys of copied rows; a key changes when the row's hours change. */
  readonly copied = this.copiedState.asReadonly();

  constructor() {
    if (!isTauri()) {
      return;
    }
    const unlisten = this.commands.listen<boolean>(TauriEvent.SummaryVisibility, (open) =>
      this.openState.set(open),
    );
    inject(DestroyRef).onDestroy(() => void unlisten.then((fn) => fn()));
    void this.commands.isSummaryOpen().then((open) => this.openState.set(open));
  }

  markCopied(...keys: string[]): void {
    this.copiedState.update((copied) => new Set([...copied, ...keys]));
  }
}

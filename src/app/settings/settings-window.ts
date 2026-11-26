import {
  ChangeDetectionStrategy,
  Component,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import {
  AbstractControl,
  NonNullableFormBuilder,
  ReactiveFormsModule,
  ValidationErrors,
  Validators,
} from '@angular/forms';

import { formatMinutes, parseMinutes } from '../core/format';
import {
  MAX_OPACITY,
  MIN_OPACITY,
  TARGET_STEP_MINUTES,
  type AccountInfo,
  type Settings,
  type SettingsView,
} from '../core/models';
import { TauriCommands, toAppError } from '../core/tauri-commands';
import { isTauri } from '../core/tauri-env';

export const WEEKDAYS = [
  'Maanantai',
  'Tiistai',
  'Keskiviikko',
  'Torstai',
  'Perjantai',
  'Lauantai',
  'Sunnuntai',
] as const;

interface Feedback {
  kind: 'success' | 'error';
  text: string;
}

const PRODUCT_NAMES: Record<AccountInfo['flavor'], string> = {
  track: 'Toggl Track',
  focus: 'Toggl 2.0',
};

/** E.g. `Toggl 2.0 · workspace 21925` or `Toggl Track · Matti · Oma`. */
export function describeAccount(account: AccountInfo): string {
  const workspace = account.workspaceName ?? `workspace ${account.workspaceId}`;
  return [PRODUCT_NAMES[account.flavor], account.name, workspace]
    .filter((part): part is string => !!part)
    .join(' · ');
}

/** A duration of whole or half hours, e.g. `7:30` or `6`. */
export function targetValidator(control: AbstractControl<string>): ValidationErrors | null {
  const minutes = parseMinutes(control.value);
  if (minutes === null) {
    return { duration: true };
  }
  return minutes % TARGET_STEP_MINUTES === 0 ? null : { halfHour: true };
}

/** Settings window: API token, daily targets and display options. */
@Component({
  selector: 'app-settings-window',
  imports: [ReactiveFormsModule],
  templateUrl: './settings-window.html',
  styleUrl: './settings-window.scss',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsWindow implements OnInit {
  private readonly commands = inject(TauriCommands);
  private readonly fb = inject(NonNullableFormBuilder);

  protected readonly weekdays = WEEKDAYS;
  protected readonly minOpacity = MIN_OPACITY;
  protected readonly maxOpacity = MAX_OPACITY;

  // Each <form> needs its own [formGroup]; without the directive, (ngSubmit)
  // never fires and the browser's native submit reloads the page.
  protected readonly tokenForm = this.fb.group({ token: this.fb.control('') });
  protected readonly form = this.fb.group({
    dailyTargets: this.fb.array(
      WEEKDAYS.map(() => this.fb.control('7:30', [Validators.required, targetValidator])),
    ),
    useSystemTimezone: this.fb.control(true),
    timezone: this.fb.control(''),
    showSeconds: this.fb.control(true),
    autostart: this.fb.control(false),
    windowOpacity: this.fb.control(MAX_OPACITY),
    autoShowSummary: this.fb.control(true),
  });

  protected readonly account = signal<AccountInfo | null>(null);
  protected readonly hasToken = signal(false);
  protected readonly systemTimezone = signal('');
  protected readonly timezones = signal<string[]>([]);
  protected readonly busy = signal(false);
  protected readonly tokenFeedback = signal<Feedback | null>(null);
  protected readonly saveFeedback = signal<Feedback | null>(null);
  protected readonly connectionText = computed(() => {
    const account = this.account();
    return account ? describeAccount(account) : null;
  });

  /** Settings fields that the form does not edit are preserved from here. */
  private loaded: Settings | null = null;

  async ngOnInit(): Promise<void> {
    if (!isTauri()) {
      return;
    }
    try {
      const [view, timezones] = await Promise.all([
        this.commands.getSettings(),
        this.commands.listTimezones(),
      ]);
      this.timezones.set(timezones);
      this.applyView(view);
    } catch (err) {
      this.saveFeedback.set({ kind: 'error', text: toAppError(err).message });
    }
  }

  protected async saveToken(): Promise<void> {
    const token = this.tokenForm.controls.token.value.trim();
    if (!token) {
      return;
    }
    await this.run(this.tokenFeedback, async () => {
      const account = await this.commands.setApiToken(token);
      this.tokenForm.reset();
      this.account.set(account);
      this.hasToken.set(true);
      return 'Avain tallennettu Keychainiin.';
    });
  }

  protected async testConnection(): Promise<void> {
    await this.run(this.tokenFeedback, async () => {
      const account = await this.commands.testConnection();
      this.account.set(account);
      return 'Yhteys toimii.';
    });
  }

  protected async clearToken(): Promise<void> {
    await this.run(this.tokenFeedback, async () => {
      await this.commands.clearApiToken();
      this.account.set(null);
      this.hasToken.set(false);
      return 'Avain poistettu.';
    });
  }

  protected async save(): Promise<void> {
    if (this.form.invalid || !this.loaded) {
      this.form.markAllAsTouched();
      return;
    }
    const value = this.form.getRawValue();
    const settings: Settings = {
      ...this.loaded,
      dailyTargets: value.dailyTargets.map((text) => parseMinutes(text) ?? 0),
      timezone: value.useSystemTimezone ? null : value.timezone.trim() || null,
      showSeconds: value.showSeconds,
      autostart: value.autostart,
      windowOpacity: value.windowOpacity,
      autoShowSummary: value.autoShowSummary,
    };
    await this.run(this.saveFeedback, async () => {
      this.loaded = await this.commands.saveSettings(settings);
      this.form.markAsPristine();
      return 'Asetukset tallennettu.';
    });
  }

  private applyView(view: SettingsView): void {
    const { settings } = view;
    this.loaded = settings;
    this.account.set(view.account);
    this.hasToken.set(view.hasToken);
    this.systemTimezone.set(view.systemTimezone);
    this.form.setValue({
      dailyTargets: settings.dailyTargets.map(formatMinutes),
      useSystemTimezone: settings.timezone === null,
      timezone: settings.timezone ?? view.systemTimezone,
      showSeconds: settings.showSeconds,
      autostart: settings.autostart,
      windowOpacity: settings.windowOpacity,
      autoShowSummary: settings.autoShowSummary,
    });
  }

  /** Runs an action with a busy flag and reports the outcome. */
  private async run(
    feedback: ReturnType<typeof signal<Feedback | null>>,
    action: () => Promise<string>,
  ): Promise<void> {
    this.busy.set(true);
    feedback.set(null);
    try {
      feedback.set({ kind: 'success', text: await action() });
    } catch (err) {
      feedback.set({ kind: 'error', text: toAppError(err).message });
    } finally {
      this.busy.set(false);
    }
  }
}

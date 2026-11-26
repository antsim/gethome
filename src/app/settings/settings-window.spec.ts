import { TestBed } from '@angular/core/testing';
import { FormControl } from '@angular/forms';

import type { AccountInfo } from '../core/models';
import { TauriCommands } from '../core/tauri-commands';

import { SettingsWindow, describeAccount, targetValidator } from './settings-window';

describe('SettingsWindow', () => {
  const account: AccountInfo = {
    flavor: 'focus',
    name: null,
    workspaceId: 21925,
    workspaceName: null,
    organizationId: 1,
  };
  let setApiToken: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    setApiToken = vi.fn().mockResolvedValue(account);
    await TestBed.configureTestingModule({
      imports: [SettingsWindow],
      providers: [{ provide: TauriCommands, useValue: { setApiToken } }],
    }).compileComponents();
  });

  it('saves the API token when the form is submitted', async () => {
    const fixture = TestBed.createComponent(SettingsWindow);
    await fixture.whenStable();
    const element = fixture.nativeElement as HTMLElement;

    const input = element.querySelector<HTMLInputElement>('#token')!;
    input.value = '  toggl_sk_abc  ';
    input.dispatchEvent(new Event('input'));
    await fixture.whenStable();

    element.querySelector<HTMLButtonElement>('.token-row button[type="submit"]')!.click();
    await fixture.whenStable();

    expect(setApiToken).toHaveBeenCalledWith('toggl_sk_abc');
    expect(element.textContent).toContain('Avain tallennettu');
    expect(element.textContent).toContain('Toggl 2.0 · workspace 21925');
  });
});

describe('describeAccount', () => {
  it('describes a Toggl Track account with name and workspace', () => {
    expect(
      describeAccount({
        flavor: 'track',
        name: 'Matti',
        workspaceId: 7,
        workspaceName: 'Oma',
        organizationId: null,
      }),
    ).toBe('Toggl Track · Matti · Oma');
  });
});

describe('targetValidator', () => {
  const check = (value: string) => targetValidator(new FormControl(value, { nonNullable: true }));

  it('accepts whole and half hours', () => {
    expect(check('7:30')).toBeNull();
    expect(check('6')).toBeNull();
    expect(check('0')).toBeNull();
  });

  it('rejects other minutes and invalid input', () => {
    expect(check('7:45')).toEqual({ halfHour: true });
    expect(check('abc')).toEqual({ duration: true });
  });
});

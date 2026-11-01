import { readFile } from 'node:fs/promises';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { isMacOS, isWindows, showsSettingsShortcut } from '../src/lib/platform';
import { TitleBar } from '../src/shell/TitleBar';

vi.mock('../src/dev/scenario', () => ({
  devParams: () => ({ mock: false, os: 'macos' }),
}));
vi.mock('../src/shell/AccountButton', () => ({
  AccountButton: () => <span data-testid="account-entry" />,
}));
vi.mock('../src/shell/WindowControls', () => ({
  WindowControls: () => <span data-testid="window-controls" />,
}));

function setPlatform(os: 'windows' | 'macos' | 'linux'): void {
  vi.stubGlobal('document', { documentElement: { dataset: { os } } });
}

afterEach(() => vi.unstubAllGlobals());

describe('shell platform layout', () => {
  it('keeps Windows and macOS menu behavior while enabling their settings shortcut', () => {
    setPlatform('macos');
    expect(isMacOS()).toBe(true);
    expect(isWindows()).toBe(false);
    expect(showsSettingsShortcut()).toBe(true);

    setPlatform('windows');
    expect(isMacOS()).toBe(false);
    expect(isWindows()).toBe(true);
    expect(showsSettingsShortcut()).toBe(true);

    setPlatform('linux');
    expect(showsSettingsShortcut()).toBe(false);
  });

  it('marks the brand group for macOS-only visual centering', async () => {
    const html = renderToStaticMarkup(<TitleBar />);
    expect(html).toContain('titlebar-brand');

    const css = await readFile(new URL('../src/styles/app.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /\[data-os='macos'\] \.titlebar-brand\s*\{[\s\S]*?position: absolute;[\s\S]*?left: 50%;[\s\S]*?transform: translateX\(-50%\);/,
    );
  });
});

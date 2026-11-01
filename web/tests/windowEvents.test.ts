import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import { CONTRACT_EVENTS, createTauriBackend } from '../src/lib/tauri-backend';

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }));

describe('window event ownership', () => {
  beforeEach(() => vi.clearAllMocks());

  it('does not execute main-window actions again in preferences', async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({ label: 'preferences' } as ReturnType<typeof getCurrentWindow>);
    const backend = createTauriBackend();
    for (const event of [CONTRACT_EVENTS.shellMenu, CONTRACT_EVENTS.shellDrop, CONTRACT_EVENTS.shellQuitRequested]) {
      backend.on(event, vi.fn());
    }
    await vi.waitFor(() => expect(getCurrentWindow).toHaveBeenCalledTimes(3));
    expect(listen).not.toHaveBeenCalled();
    backend.on(CONTRACT_EVENTS.settingsChanged, vi.fn());
    await vi.waitFor(() => expect(listen).toHaveBeenCalledTimes(1));
    expect(listen).toHaveBeenCalledWith(CONTRACT_EVENTS.settingsChanged, expect.any(Function), { target: 'preferences' });
  });

  it('binds main-window commands to the main event target', async () => {
    vi.mocked(getCurrentWindow).mockReturnValue({ label: 'main' } as ReturnType<typeof getCurrentWindow>);
    const handler = vi.fn();
    createTauriBackend().on(CONTRACT_EVENTS.shellMenu, handler);
    await vi.waitFor(() => expect(listen).toHaveBeenCalledTimes(1));
    expect(listen).toHaveBeenCalledWith(CONTRACT_EVENTS.shellMenu, expect.any(Function), { target: 'main' });
    const listener = vi.mocked(listen).mock.calls[0][1];
    listener({ event: CONTRACT_EVENTS.shellMenu, id: 1, payload: { id: 'open-link' } });
    expect(handler).toHaveBeenCalledExactlyOnceWith({ id: 'open-link' });
  });
});

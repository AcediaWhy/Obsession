import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
const state = vi.hoisted(() => ({ hidden: true, listener: null as null | (() => void), emit: vi.fn(async () => {}) }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => true }));
vi.mock('@tauri-apps/api/event', () => ({ emit: state.emit }));
vi.mock('./render', () => ({ renderHidden: () => state.hidden, onRenderActiveChange: (cb: () => void) => { state.listener = cb; return () => { state.listener = null; }; } }));
import { initTraySleep } from './traySleep';
import { backendIdle, trackBackendRequest } from '../lib/backendActivity';
beforeEach(() => { vi.useFakeTimers(); state.hidden = true; state.emit.mockClear(); });
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe('tray sleep protects backend work', () => {
  it('waits for chained config probes and preserves their results', async () => {
    const stop = initTraySleep();
    let finish!: (value: number) => void;
    const work = trackBackendRequest(() => new Promise<number>(resolve => { finish = resolve; }));
    vi.advanceTimersByTime(30000); expect(state.emit).not.toHaveBeenCalled(); expect(backendIdle()).toBe(false);
    finish(42); expect(await work).toBe(42);
    const second = trackBackendRequest(async () => 43); expect(await second).toBe(43);
    vi.advanceTimersByTime(2999); expect(state.emit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1); expect(state.emit).toHaveBeenCalledWith('ui-tray-idle'); stop();
  });
  it('unblocks sleep after rejection without swallowing the error', async () => {
    const stop = initTraySleep(); const error = new Error('probe failed');
    await expect(trackBackendRequest(async () => { throw error; })).rejects.toBe(error);
    expect(backendIdle()).toBe(true); vi.advanceTimersByTime(3000);
    expect(state.emit).toHaveBeenCalledOnce(); stop();
  });
  it('does not sleep until all concurrent requests complete', async () => {
    const stop = initTraySleep(); let finish!: () => void;
    const slow = trackBackendRequest(() => new Promise<void>(resolve => { finish = resolve; }));
    await trackBackendRequest(async () => undefined); vi.advanceTimersByTime(5000);
    expect(state.emit).not.toHaveBeenCalled(); finish(); await slow;
    vi.advanceTimersByTime(3000); expect(state.emit).toHaveBeenCalledOnce(); stop();
  });
  it('cancels sleep when the window opens or the owner unmounts', () => {
    const stop = initTraySleep(); vi.advanceTimersByTime(2000);
    state.hidden = false; state.listener?.(); vi.advanceTimersByTime(5000);
    expect(state.emit).not.toHaveBeenCalled();
    state.hidden = true; state.listener?.(); stop(); vi.advanceTimersByTime(5000);
    expect(state.emit).not.toHaveBeenCalled();
  });
});

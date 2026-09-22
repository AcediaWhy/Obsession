import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';

const state = vi.hoisted(() => ({ hidden: false, listener: null as null | (() => void), releases: Array.from({ length: 5 }, () => vi.fn()) }));
vi.mock('../render', () => ({ renderHidden: () => state.hidden, onRenderActiveChange: (cb: () => void) => { state.listener = cb; return () => { state.listener = null; }; } }));
vi.mock('../videoPool', () => ({ resetVideoPool: state.releases[0] }));
vi.mock('../components/obsessionChoir/fieldSession', () => ({ choirFieldSession: { release: state.releases[1] } }));
vi.mock('../components/rain/fieldSession', () => ({ rainFieldSession: { release: state.releases[2] } }));
vi.mock('../components/YaniCharacterScene', () => ({ releaseYaniStages: state.releases[3] }));
vi.mock('../components/yanineko/fieldSession', () => ({ yaniFieldSession: { release: state.releases[4] } }));
import { initTrayStageRelease } from './trayStageRelease';

beforeEach(() => { vi.useFakeTimers(); state.hidden = false; state.releases.forEach(fn => fn.mockClear()); });
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe('tray-wide graphics cleanup', () => {
  it('does not release a visible reduced-motion scene', () => {
    const stop = initTrayStageRelease(); state.listener?.(); vi.advanceTimersByTime(10000);
    state.releases.forEach(fn => expect(fn).not.toHaveBeenCalled()); stop();
  });
  it('releases all pools once, including startup in the tray', () => {
    state.hidden = true; const stop = initTrayStageRelease(); vi.advanceTimersByTime(1000);
    state.listener?.(); vi.advanceTimersByTime(10000);
    state.releases.forEach(fn => expect(fn).toHaveBeenCalledOnce()); stop();
  });
  it('cancels when restored early and rearms for the next tray cycle', () => {
    const stop = initTrayStageRelease(); state.hidden = true; state.listener?.(); vi.advanceTimersByTime(500);
    state.hidden = false; state.listener?.(); vi.advanceTimersByTime(2000);
    state.releases.forEach(fn => expect(fn).not.toHaveBeenCalled());
    state.hidden = true; state.listener?.(); vi.advanceTimersByTime(1000);
    state.releases.forEach(fn => expect(fn).toHaveBeenCalledOnce()); stop();
  });
  it('cancels delayed cleanup when the owner unmounts', () => {
    state.hidden = true; const stop = initTrayStageRelease(); stop(); vi.advanceTimersByTime(10000);
    state.releases.forEach(fn => expect(fn).not.toHaveBeenCalled());
  });
});

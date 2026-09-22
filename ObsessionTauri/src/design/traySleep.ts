import { isTauri } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { backendIdle, onBackendActivity } from '../lib/backendActivity';
import { onRenderActiveChange, renderHidden } from './render';

// Requests may form async chains (manual config search, onboarding, saves).
// Three idle seconds allow their continuations and graphics cleanup to finish.
export function initTraySleep(): () => void {
  if (!isTauri()) return () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  const update = () => {
    clearTimeout(timer); timer = undefined;
    if (renderHidden() && backendIdle()) timer = setTimeout(() => {
      timer = undefined;
      if (renderHidden() && backendIdle()) void emit('ui-tray-idle').catch(() => {});
    }, 3000);
  };
  const unlistenRender = onRenderActiveChange(update);
  const unlistenActivity = onBackendActivity(update);
  update();
  return () => { clearTimeout(timer); unlistenRender(); unlistenActivity(); };
}

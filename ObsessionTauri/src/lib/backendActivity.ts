let pending = 0;
const listeners = new Set<() => void>();
export function backendIdle() { return pending === 0; }
export function onBackendActivity(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
function notify() { listeners.forEach(listener => listener()); }
export async function trackBackendRequest<T>(request: () => Promise<T>): Promise<T> {
  pending++; notify();
  try { return await request(); }
  finally { pending--; notify(); }
}

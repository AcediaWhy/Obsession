import { useEffect, useState } from "react";
import { api, type SystemDnsStatus } from "../../lib/tauri";
import { useSettingsStore } from "../../store/settingsStore";
import { Button } from "./atoms";

export function SystemDnsPanel() {
  const available = useSettingsStore((s) => s.protectedRuntime.systemDnsRecovery);
  const [status, setStatus] = useState<SystemDnsStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [restored, setRestored] = useState(false);

  useEffect(() => {
    if (!available) return;
    let current = true;
    api.systemDnsStatus().then((value) => { if (current) { setStatus(value); setError(""); } })
      .catch((err) => { if (current) setError(String(err)); });
    return () => { current = false; };
  }, [available]);

  async function restore() {
    setBusy(true); setError(""); setRestored(false);
    try { setStatus(await api.systemDnsRestore()); setRestored(true); }
    catch (err) { setError(String(err)); }
    finally { setBusy(false); }
  }

  return <div className="flex flex-col gap-3 text-sm">
    <p className="text-ink-muted">
      Здесь можно вручную отменить изменение DNS, сделанное предыдущей версией Obsession.
    </p>
    {!available ? null
      : status?.active ? <p className="text-warn">Осталось изменение предыдущей версии: {status.provider === "google" ? "Google" : "Cloudflare"} · {status.interfaceName}</p>
      : status?.restoreAvailable ? <p className="text-warn">Есть старый журнал отката. Если DNS уже исправлен вручную, повторное восстановление не требуется.</p>
      : <p className="text-ink-soft">{restored ? "Исходный DNS восстановлен." : status ? "Используются текущие настройки Windows." : "Проверяем настройки DNS…"}</p>}
    {error && <p className="text-danger" role="alert">{error}</p>}
    {status?.restoreAvailable && <div>
      <Button variant="ghost" disabled={!status?.restoreAvailable || busy} onClick={restore}>
        {busy ? "Восстановление…" : "Восстановить исходный DNS"}
      </Button>
    </div>}
  </div>;
}

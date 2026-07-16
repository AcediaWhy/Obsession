import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";

import { api, type DiagResult } from "../../lib/tauri";
import { Button, SectionLabel } from "./atoms";
import { Icon } from "./icons";
import { spring } from "../tokens";

// Виджет диагностики: полноценный HTTPS-GET к заблокированным ресурсам через
// бэкенд. Отвечает на главный вопрос пользователя — «обход реально работает?».
export function Diagnostics() {
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<DiagResult[] | null>(null);
  // diagnose() — секундные HTTPS-пробы; уход с экрана DPI до их конца не должен
  // звать setState на размонтированном компоненте (ref, т.к. это обработчик, а
  // не эффект — привязать очистку к жизненному циклу иначе нельзя).
  const mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);

  const run = async () => {
    setRunning(true);
    try {
      const r = await api.diagnose();
      if (mounted.current) setResults(r);
    } catch {
      if (mounted.current) setResults([]);
    } finally {
      if (mounted.current) setRunning(false);
    }
  };

  const okCount = results?.filter((r) => r.ok).length ?? 0;
  const total = results?.length ?? 0;

  return (
    <div className="w-full">
      <div className="mb-2 flex items-center justify-between">
        <SectionLabel>Диагностика</SectionLabel>
        {results && (
          <span
            className={`text-2xs font-semibold tabular-nums ${
              okCount === total ? "text-ok" : okCount === 0 ? "text-danger" : "text-warn"
            }`}
          >
            {okCount}/{total} доступны
          </span>
        )}
      </div>

      <div className="flex flex-col gap-2">
        <Button variant="ghost" disabled={running} onClick={run} className="w-full">
          <span className="flex items-center justify-center gap-1.5">
            <Icon.Refresh size={15} />
            {running ? "Проверка…" : "Проверить доступность"}
          </span>
        </Button>

        <AnimatePresence initial={false}>
          {results && results.length > 0 && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={spring.expand}
              className="flex flex-col gap-1 overflow-hidden"
            >
              {results.map((r) => (
                <div
                  key={r.name}
                  className="flex items-center justify-between rounded-lg bg-white/5 px-3 py-2 text-sm"
                >
                  <span className="flex items-center gap-2">
                    <span
                      className={[
                        "h-2 w-2 rounded-full",
                        r.ok
                          ? "bg-ok shadow-[0_0_8px_1px_rgba(52,211,153,0.7)]"
                          : "bg-danger shadow-[0_0_8px_1px_rgba(248,113,113,0.7)]",
                      ].join(" ")}
                    />
                    <span className="text-ink-soft">{r.name}</span>
                  </span>
                  <span className={`text-2xs tabular-nums ${r.ok ? "text-ink-muted" : "text-danger"}`}>
                    {r.ok ? `${r.ms} мс` : "недоступен"}
                  </span>
                </div>
              ))}
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

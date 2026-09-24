import { useEffect, useState, type ReactNode } from "react";
import { Toaster as SonnerToaster, toast } from "sonner";

import { Icon } from "./pixelIcons";

// Sonner управляет очередью, автоскрытием, жестами и доступностью уведомлений.
// Разметку и стили задаёт лаборатория; её уведомления отделены от useToastStore.

export type LabToastKind = "success" | "info" | "warn" | "error";

const TONE: Record<LabToastKind, { icon: ReactNode; title: string }> = {
  success: { icon: <Icon.Check size={16} />, title: "готово" },
  info: { icon: <Icon.Info size={16} />, title: "сообщение" },
  warn: { icon: <Icon.Alert size={16} />, title: "внимание" },
  error: { icon: <Icon.Danger size={16} />, title: "сбой" },
};

function PixelToast({
  id,
  kind,
  message,
}: {
  id: string | number;
  kind: LabToastKind;
  message: string;
}) {
  return (
    <div className="sunken-lab-toast" data-kind={kind}>
      <span className="sunken-lab-toast__icon">{TONE[kind].icon}</span>
      <div className="sunken-lab-toast__body">
        <b>{TONE[kind].title}</b>
        <p>{message}</p>
      </div>
      <button aria-label="Закрыть" onClick={() => toast.dismiss(id)} type="button">
        <Icon.X size={16} />
      </button>
    </div>
  );
}

function push(kind: LabToastKind, message: string) {
  return toast.custom((id) => <PixelToast id={id} kind={kind} message={message} />, {
    duration: kind === "error" ? 6000 : 4200,
  });
}

export const labToast = {
  success: (message: string) => push("success", message),
  info: (message: string) => push("info", message),
  warn: (message: string) => push("warn", message),
  error: (message: string) => push("error", message),
};

/**
 * Обёртка нужна из-за SSR: sonner читает `document.hidden` прямо в теле рендера
 * (useIsDocumentHidden), поэтому на сервере её Toaster падает. Тесты лабы гоняют
 * renderToStaticMarkup в node-окружении, так что настоящий контейнер монтируем
 * только после гидрации, а стабильный якорь отдаём всегда.
 */
export function PixelToaster() {
  const [mounted, setMounted] = useState(false);
  useEffect(() => setMounted(true), []);

  return (
    <div data-px-toaster>
      {mounted ? (
        <SonnerToaster
          gap={8}
          offset={18}
          position="bottom-right"
          toastOptions={{ unstyled: true, classNames: { toast: "sunken-lab-toast-slot" } }}
          visibleToasts={4}
        />
      ) : null}
    </div>
  );
}

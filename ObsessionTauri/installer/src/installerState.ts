export type InstallerErrorCode =
  | "BUSY"
  | "UAC_CANCELLED"
  | "DOWNGRADE_BLOCKED"
  | "PREFLIGHT_FAILED"
  | "INSTALL_FAILED"
  | "ROLLBACK_RESTORED"
  | "ROLLBACK_INCOMPLETE"
  | "LAUNCH_FAILED";

export type InstallerFailure = {
  code: InstallerErrorCode;
  retryable: boolean;
  messageCode: string;
  logPath: string | null;
};

export type InstallerOutcome = {
  dir: string;
  mode: "install" | "update" | "repair";
  version: string;
};

export type InstallerProgressEvent = {
  sequence: number;
  pct: number;
  stage: string;
};

export type InstallerProgressState = InstallerProgressEvent;

export type InstallerFailureCopy = {
  eyebrow: string;
  title: string;
  summary: string;
  action: string;
};

const ERROR_CODES = new Set<InstallerErrorCode>([
  "BUSY",
  "UAC_CANCELLED",
  "DOWNGRADE_BLOCKED",
  "PREFLIGHT_FAILED",
  "INSTALL_FAILED",
  "ROLLBACK_RESTORED",
  "ROLLBACK_INCOMPLETE",
  "LAUNCH_FAILED",
]);

const STAGE_ORDER = new Map(
  ["prepare", "stop", "stage", "install", "verify", "commit", "registry", "shortcuts", "cleanup", "finish"].map(
    (stage, index) => [stage, index],
  ),
);

const FAILURE_COPY: Record<InstallerErrorCode, InstallerFailureCopy> = {
  BUSY: {
    eyebrow: "Установщик уже занят",
    title: "Другая операция ещё выполняется",
    summary: "Дождитесь завершения уже запущенной установки или восстановления и повторите попытку.",
    action: "Проверить снова",
  },
  UAC_CANCELLED: {
    eyebrow: "Запрос прав отменён",
    title: "Права не предоставлены",
    summary: "Windows не разрешила запустить защищённый worker. Файлы приложения не изменялись.",
    action: "Повторить запрос",
  },
  DOWNGRADE_BLOCKED: {
    eyebrow: "Защита от понижения версии",
    title: "Установлена более новая версия",
    summary: "Этот setup старее уже установленного Obsession. Файлы не изменялись.",
    action: "",
  },
  PREFLIGHT_FAILED: {
    eyebrow: "Проверка остановлена",
    title: "Система пока не готова к установке",
    summary: "Безопасная предварительная проверка не пройдена. Подробности сохранены в локальном журнале.",
    action: "Проверить снова",
  },
  INSTALL_FAILED: {
    eyebrow: "Установка остановлена",
    title: "Не удалось завершить операцию",
    summary: "Изменение не было подтверждено. Технические подробности сохранены только в локальном журнале.",
    action: "Попробовать снова",
  },
  ROLLBACK_RESTORED: {
    eyebrow: "Изменение отменено",
    title: "Предыдущая версия восстановлена",
    summary: "Новая версия не прошла проверку, поэтому setup вернул последнюю рабочую установку.",
    action: "Попробовать снова",
  },
  ROLLBACK_INCOMPLETE: {
    eyebrow: "Требуется восстановление",
    title: "Автоматический возврат не завершён",
    summary: "Обычный повтор заблокирован, чтобы не повредить установку. Закройте setup и запустите его снова для recovery.",
    action: "",
  },
  LAUNCH_FAILED: {
    eyebrow: "Установка завершена",
    title: "Не удалось открыть Obsession",
    summary: "Файлы уже установлены. Попробуйте запуск ещё раз или откройте Obsession через меню «Пуск».",
    action: "Запустить снова",
  },
};

export const initialProgressState: InstallerProgressState = {
  sequence: 0,
  pct: 0,
  stage: "prepare",
};

export function applyProgressEvent(
  current: InstallerProgressState,
  event: InstallerProgressEvent,
): InstallerProgressState {
  if (
    !Number.isSafeInteger(event.sequence) ||
    event.sequence <= current.sequence ||
    !Number.isFinite(event.pct) ||
    event.pct < 0 ||
    event.pct > 100 ||
    !Number.isInteger(event.pct) ||
    (!STAGE_ORDER.has(event.stage) && event.stage !== "rollback")
  ) {
    return current;
  }

  const currentOrder = STAGE_ORDER.get(current.stage) ?? -1;
  const nextOrder = STAGE_ORDER.get(event.stage) ?? currentOrder;
  if (event.stage !== "rollback" && (event.pct < current.pct || nextOrder < currentOrder)) {
    return current;
  }

  return event;
}

export function installerFailureCopy(code: InstallerErrorCode): InstallerFailureCopy {
  return FAILURE_COPY[code];
}

export function normalizeInstallerFailure(
  value: unknown,
  fallbackCode: InstallerErrorCode,
): InstallerFailure {
  if (value && typeof value === "object") {
    const candidate = value as Partial<InstallerFailure>;
    if (
      typeof candidate.code === "string" &&
      ERROR_CODES.has(candidate.code as InstallerErrorCode) &&
      typeof candidate.retryable === "boolean" &&
      typeof candidate.messageCode === "string" &&
      (candidate.logPath === null || typeof candidate.logPath === "string")
    ) {
      return candidate as InstallerFailure;
    }
  }

  return {
    code: fallbackCode,
    retryable: fallbackCode !== "DOWNGRADE_BLOCKED" && fallbackCode !== "ROLLBACK_INCOMPLETE",
    messageCode: `installer.error.${fallbackCode.toLowerCase()}`,
    logPath: null,
  };
}

export function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[character];
  });
}

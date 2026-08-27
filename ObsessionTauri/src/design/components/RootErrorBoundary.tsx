import { Component, type ErrorInfo, type ReactNode } from "react";

// Раньше любой бросок в оболочке размонтировал ВСЁ дерево: окно уходило в фон
// body без сообщения и без единой кнопки, и причину приходилось искать в
// devtools (так было с порядком хуков в NavRail — экран просто темнел). Граница
// ловит ошибку рендера, показывает текст со стеком компонентов и даёт
// перезагрузить окно.
//
// Стили только инлайновые, без классов и токенов: сбой может быть в самом CSS
// (упавший PostCSS уже оставлял приложение с обрезанной таблицей стилей), и
// экран ошибки не должен зависеть от того, что доехало.

type AsyncErrorRecord = { at: string; kind: "error" | "rejection"; message: string };

const MAX_RECORDS = 20;

/** Асинхронные ошибки границами React не ловятся — складываем их в буфер, чтобы
 *  в следующий раз не гадать. Поведение приложения не меняем: не фатально. */
function installAsyncErrorRecorder(): AsyncErrorRecord[] {
  const store = window as unknown as { __obsessionErrors?: AsyncErrorRecord[] };
  if (store.__obsessionErrors) return store.__obsessionErrors;
  const records: AsyncErrorRecord[] = [];
  store.__obsessionErrors = records;
  const push = (kind: AsyncErrorRecord["kind"], message: string) => {
    records.push({ at: new Date().toISOString(), kind, message });
    if (records.length > MAX_RECORDS) records.shift();
  };
  window.addEventListener("error", (event) => {
    push("error", event.message || String(event.error ?? "unknown"));
  });
  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    push("rejection", reason instanceof Error ? reason.message : String(reason));
  });
  return records;
}

type Props = { children: ReactNode };
type State = { error: Error | null; componentStack: string };

export class RootErrorBoundary extends Component<Props, State> {
  state: State = { error: null, componentStack: "" };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidMount() {
    installAsyncErrorRecorder();
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ componentStack: info.componentStack ?? "" });
    console.error("Obsession: необработанная ошибка рендера", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <ErrorScreen error={this.state.error} componentStack={this.state.componentStack} />
    );
  }
}

const PANEL: Record<string, string | number> = {
  position: "fixed",
  inset: 0,
  zIndex: 2147483647,
  display: "flex",
  flexDirection: "column",
  gap: "14px",
  padding: "28px 30px",
  overflow: "auto",
  background: "#0b0d12",
  color: "#e6ebf2",
  font: "13px/1.5 ui-monospace, 'IBM Plex Mono', Consolas, monospace",
};

const BUTTON: Record<string, string | number> = {
  padding: "7px 14px",
  border: "1px solid #2f3a4a",
  borderRadius: "6px",
  background: "#141924",
  color: "#e6ebf2",
  font: "inherit",
  cursor: "pointer",
};

const BLOCK: Record<string, string | number> = {
  margin: 0,
  padding: "12px 14px",
  maxHeight: "34vh",
  overflow: "auto",
  border: "1px solid #202836",
  borderRadius: "8px",
  background: "#070910",
  whiteSpace: "pre-wrap",
  wordBreak: "break-word",
};

export function ErrorScreen({
  error,
  componentStack,
}: {
  error: Error;
  componentStack: string;
}) {
  // typeof window: компонент рендерится и через renderToStaticMarkup в тестах,
  // где DOM нет (vitest environment: node).
  const asyncRecords = (
    typeof window === "undefined"
      ? []
      : (window as unknown as { __obsessionErrors?: AsyncErrorRecord[] }).__obsessionErrors ?? []
  )
    .map((r) => `${r.at} [${r.kind}] ${r.message}`)
    .join("\n");
  const report = [
    `${error.name}: ${error.message}`,
    error.stack ?? "",
    componentStack ? `Стек компонентов:${componentStack}` : "",
    asyncRecords ? `Асинхронные ошибки:\n${asyncRecords}` : "",
  ]
    .filter(Boolean)
    .join("\n\n");

  return (
    <div style={PANEL} role="alert">
      <div style={{ fontSize: "15px", fontWeight: 600 }}>Интерфейс упал</div>
      <div style={{ color: "#9aa7b8" }}>
        Обход и прокси продолжают работать — сбой только в окне. Скопируй отчёт и
        перезагрузи окно.
      </div>
      <pre style={BLOCK}>{report}</pre>
      <div style={{ display: "flex", gap: "10px" }}>
        <button style={BUTTON} type="button" onClick={() => location.reload()}>
          Перезагрузить окно
        </button>
        <button
          style={BUTTON}
          type="button"
          onClick={() => void navigator.clipboard?.writeText(report)}
        >
          Скопировать отчёт
        </button>
      </div>
    </div>
  );
}

import { win } from "../../lib/tauri";
import { setWindowShown } from "../render";

// Кастомный титлбар: drag-регион + кнопки управления окном.
export function CustomTitleBar() {
  // Оптимистичная пауза: гасим анимации сразу по клику на сворачивание, не дожидаясь
  // Rust-поллера (~300мс). Поллер затем подтверждает состояние и вернёт shown=true
  // при разворачивании — он остаётся источником правды.
  const minimize = () => {
    setWindowShown(false);
    void win.minimize();
  };

  return (
    <div className="drag-region flex h-10 items-center justify-between px-4">
      <div className="flex items-center gap-2">
        <div className="h-2.5 w-2.5 rounded-full bg-accent shadow-glow" />
        <span className="text-xs font-semibold tracking-wide text-ink-soft">
          Obsession
        </span>
      </div>
      <div className="no-drag flex items-center gap-1">
        <WinButton label="—" ariaLabel="Свернуть" onClick={minimize} />
        <WinButton label="▢" ariaLabel="Развернуть" onClick={() => win.toggleMaximize()} small />
        <WinButton label="✕" ariaLabel="Закрыть" onClick={() => win.close()} danger />
      </div>
    </div>
  );
}

function WinButton({
  label,
  ariaLabel,
  onClick,
  danger = false,
  small = false,
}: {
  label: string;
  ariaLabel: string;
  onClick: () => void;
  danger?: boolean;
  small?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      aria-label={ariaLabel}
      className={[
        "flex h-7 w-7 items-center justify-center rounded-lg text-ink-muted transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70",
        small ? "text-3xs" : "text-xs",
        danger ? "hover:bg-danger/20 hover:text-danger" : "hover:bg-white/10 hover:text-ink",
      ].join(" ")}
    >
      {label}
    </button>
  );
}

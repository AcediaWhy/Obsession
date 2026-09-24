import { win } from "../../lib/tauri";
import { setWindowShown } from "../render";

// SVG сохраняют одинаковый размер и выравнивание кнопок при смене шрифта.
const GLYPH = {
  minimize: <path d="M2 5h6" />,
  maximize: <rect x="2" y="2" width="6" height="6" rx="1" />,
  close: <path d="m2.5 2.5 5 5m0-5-5 5" />,
} as const;

function WinGlyph({ kind }: { kind: keyof typeof GLYPH }) {
  return (
    <svg
      width={10}
      height={10}
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.2}
      strokeLinecap="round"
      aria-hidden
    >
      {GLYPH[kind]}
    </svg>
  );
}

// Область перетаскивания окна и кнопки управления.
export function CustomTitleBar() {
  // При сворачивании останавливаем анимации сразу. Состояние окна затем
  // синхронизирует Rust-поллер; он же возобновит их при разворачивании.
  const minimize = () => {
    setWindowShown(false);
    void win.minimize();
  };

  return (
    <div className="theme-morph drag-region flex h-10 items-center justify-between px-4">
      <div className="flex items-center gap-2">
        <div className="h-2.5 w-2.5 rounded-full bg-accent shadow-glow" />
        <span className="text-xs font-semibold tracking-wide text-ink-soft">
          Obsession
        </span>
      </div>
      <div className="no-drag flex items-center gap-1">
        <WinButton kind="minimize" ariaLabel="Свернуть" onClick={minimize} />
        <WinButton kind="maximize" ariaLabel="Развернуть" onClick={() => win.toggleMaximize()} />
        <WinButton kind="close" ariaLabel="Закрыть" onClick={() => win.close()} danger />
      </div>
    </div>
  );
}

function WinButton({
  kind,
  ariaLabel,
  onClick,
  danger = false,
}: {
  kind: keyof typeof GLYPH;
  ariaLabel: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={ariaLabel}
      className={[
        "flex h-7 w-7 items-center justify-center rounded-lg text-ink-muted transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/70",
        danger ? "hover:bg-danger/20 hover:text-danger" : "hover:bg-white/10 hover:text-ink",
      ].join(" ")}
    >
      <WinGlyph kind={kind} />
    </button>
  );
}

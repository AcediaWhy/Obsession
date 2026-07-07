import { GlassPanel } from "../design/components/GlassPanel";

// Заглушка для функций этапа 2 (профили, списки, настройки).
export function SoonScreen({ title }: { title: string }) {
  return (
    <div className="flex h-full flex-col gap-4">
      <h1 className="font-display text-3xl font-bold text-gradient">{title}</h1>
      <GlassPanel className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
        <div className="rounded-full bg-accent/15 px-4 py-1.5 text-xs font-semibold uppercase tracking-widest text-accent">
          Скоро
        </div>
        <p className="max-w-sm text-sm text-ink-muted">
          Этот раздел появится на следующем этапе. Сейчас доступны DPI-обход,
          ИИ-разблокировка и Telegram-прокси.
        </p>
      </GlassPanel>
    </div>
  );
}

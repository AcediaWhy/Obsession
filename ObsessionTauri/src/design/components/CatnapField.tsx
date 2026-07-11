import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { VideoField } from "./VideoField";

// Фон темы «Catnap»: вагон в закатном золоте, спящий белый кот — видео-луп.
// Закат яркий, поэтому фон ощутимо приглушаем тёплым тоном (плотное стекло
// панелей — вторая половина читаемости, см. [data-theme="catnap"] в globals.css).
// Поверх — медленно дышащая диагональная полоса закатного света; при активном
// обходе/прокси свет теплеет сильнее.
export function CatnapField() {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;

  return (
    <VideoField src="/catnap/loop.mp4" poster="/catnap/poster.jpg">
      {/* Тёплое приглушение: сильнее сверху (титлбар) и снизу (контент). */}
      <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(20,10,6,0.50)_0%,rgba(20,10,6,0.18)_38%,rgba(20,10,6,0.46)_100%)]" />
      {/* Дыхание закатного света — тёплая диагональная полоса, медленный пульс. */}
      <div
        className="absolute inset-0"
        style={{
          background:
            "linear-gradient(115deg, transparent 30%, rgba(255,190,120,0.10) 45%, rgba(255,160,80,0.06) 56%, transparent 72%)",
          animation: "sun-breathe 9s ease-in-out infinite",
        }}
      />
      {/* Тёплое «дыхание» при активности. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background:
            "radial-gradient(ellipse at 50% 62%, rgba(255,178,102,0.16), transparent 60%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Мягкая тёплая виньетка. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_46%,rgba(14,7,4,0.6))]" />
    </VideoField>
  );
}

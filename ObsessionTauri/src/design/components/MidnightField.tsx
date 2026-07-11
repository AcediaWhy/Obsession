import { useDpiStore } from "../../store/dpiStore";
import { useProxyStore } from "../../store/proxyStore";
import { VideoField } from "./VideoField";

// Тайл монохромного шума для плёночного зерна (feTurbulence, бесшовный stitch).
// Инлайном как data-URI: нет лишнего запроса, тайл 160px размножается repeat'ом.
const GRAIN = `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")`;

// Фон темы «Midnight»: ч/б ночь, фонари в тумане — видео-луп. Кадр почти чёрный,
// затемнять нечего — идентичность делают плёночное зерно поверх (дискретные
// сдвиги тайла шума, как у киноплёнки) и обесцвечивающее стекло панелей
// (см. [data-theme="midnight"] в globals.css). При активном обходе/прокси туман
// у фонарей едва заметно разгорается холодным.
export function MidnightField() {
  const dpiActive = useDpiStore((s) => s.active);
  const proxyRunning = useProxyStore((s) => s.running);
  const hot = dpiActive || proxyRunning;

  return (
    <VideoField src="/midnight/loop.mp4" poster="/midnight/poster.jpg">
      {/* Холодное свечение при активности — вокруг фонарей в правой трети кадра. */}
      <div
        className="absolute inset-0 transition-opacity duration-[1600ms]"
        style={{
          background:
            "radial-gradient(ellipse at 68% 30%, rgba(190,210,235,0.10), transparent 55%)",
          opacity: hot ? 1 : 0,
        }}
      />
      {/* Мягкая виньетка. */}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_55%,rgba(0,0,0,0.5))]" />
      {/* Плёночное зерно — поверх всего. Оверскан под translate-сдвиги
          анимации (до ±30px), чтобы края не оголялись. */}
      <div
        className="absolute"
        style={{
          inset: -40,
          backgroundImage: GRAIN,
          animation: "film-grain 0.9s steps(1, end) infinite",
        }}
      />
    </VideoField>
  );
}

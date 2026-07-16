import { Component, lazy, Suspense, type ReactNode } from "react";
import type { Theme } from "../../store/themeStore";
import { useRenderActive, useRenderHidden } from "../render";
import { AuroraField } from "./AuroraField";
import { OphanimField } from "./OphanimField";
import { FallenField } from "./FallenField";
import { RainField2D } from "./RainField2D";
import { CatnapField } from "./CatnapField";
import { MidnightField } from "./MidnightField";

// 3D-сцена «Rain» (id japan) грузится лениво (three.js только при выборе темы)
// и только если доступен WebGL; при любой ошибке рендера — откат на 2D-дождь
// (RainField2D), тематически верный запасной вариант.
const RainScene3D = lazy(() => import("./RainScene3D"));

function webglSupported(): boolean {
  try {
    const c = document.createElement("canvas");
    return !!(c.getContext("webgl2") || c.getContext("webgl"));
  } catch {
    return false;
  }
}
const WEBGL = webglSupported();

// Ловим сбои 3D-сцены (драйвер/WebGL/шейдер) и показываем 2D-версию.
class Fallback3D extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(err: unknown) {
    console.warn("Rain 3D-сцена упала, откат на 2D:", err);
  }
  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

// Диспетчер реактивного фона по выбранной теме. Тема приходит ПРОПОМ (а не из
// стора): при кроссфейде смены темы AnimatePresence держит уходящую ветку с
// последним значением пропа — если бы диспетчер читал стор сам, старая ветка
// мгновенно переключилась бы на новую тему и кроссфейд выродился в кат.
export function HeroField({ theme }: { theme: Theme }) {
  // Пока окно скрыто (трей/сворачивание) — размонтируем тяжёлую WebGL-сцену Rain
  // целиком. Её rAF-циклы и так паузятся по renderActive, НО живой WebGL-контекст
  // + полноэкранные текстуры остаются на GPU и держат compositor WebView2 занятым
  // (~2-4% в трее). Размонтирование запускает RainRenderer/Raindrops.destroy()
  // (освобождает контекст/текстуры), а при возврате сцена собирается заново.
  // Лёгкие 2D-темы так не мучаем — они дёшевы и мгновенно паузятся.
  const renderOn = useRenderActive();
  const hidden = useRenderHidden();
  // В трее (suspended) снимаем ВСЮ тяжёлую сцену темы — canvas/WebGL/video.
  // Размонтирование видео-полей (catnap/midnight) запускает teardown VideoField
  // (pause + снятие src + load) и освобождает видеодекодеры; canvas-поля
  // отпускают backing stores; Rain — WebGL-контекст и текстуры. Отдаём лёгкую
  // статичную подложку: в трее webview всё равно скрыт, но она убирает тёмную
  // вспышку на первом кадре возврата, пока сцена монтируется заново.
  if (hidden) {
    return (
      <div
        className="absolute inset-0"
        style={{ background: "radial-gradient(130% 130% at 50% 0%, #0b0d12, #050609)" }}
      />
    );
  }
  if (theme === "ophanim") return <OphanimField />;
  if (theme === "fallendown") return <FallenField />;
  // Видео-темы: <video> паузится по гейту видимости внутри VideoField,
  // декодер в трее не работает — размонтировать, как Rain, не нужно.
  if (theme === "catnap") return <CatnapField />;
  if (theme === "midnight") return <MidnightField />;
  if (theme === "japan") {
    // В трее показываем лёгкий 2D-дождь-заглушку (почти бесплатен и сразу
    // паузится) вместо WebGL-сцены — визуально та же тема, без утечки контекста.
    if (!WEBGL || !renderOn) return <RainField2D />;
    return (
      <Fallback3D fallback={<RainField2D />}>
        <Suspense fallback={<RainField2D />}>
          <RainScene3D />
        </Suspense>
      </Fallback3D>
    );
  }
  return <AuroraField />;
}


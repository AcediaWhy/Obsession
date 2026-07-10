import { Component, lazy, Suspense, type ReactNode } from "react";
import { useThemeStore } from "../../store/themeStore";
import { useRenderActive } from "../render";
import { AuroraField } from "./AuroraField";
import { OphanimField } from "./OphanimField";
import { FallenField } from "./FallenField";
import { FirefliesField } from "./FirefliesField";
import { HearthField } from "./HearthField";
import { RainField2D } from "./RainField2D";

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

// Диспетчер реактивного фона по выбранной теме.
export function HeroField() {
  const theme = useThemeStore((s) => s.theme);
  // Пока окно скрыто (трей/сворачивание) — размонтируем тяжёлую WebGL-сцену Rain
  // целиком. Её rAF-циклы и так паузятся по renderActive, НО живой WebGL-контекст
  // + полноэкранные текстуры остаются на GPU и держат compositor WebView2 занятым
  // (~2-4% в трее). Размонтирование запускает RainRenderer/Raindrops.destroy()
  // (освобождает контекст/текстуры), а при возврате сцена собирается заново.
  // Лёгкие 2D-темы так не мучаем — они дёшевы и мгновенно паузятся.
  const renderOn = useRenderActive();
  if (theme === "ophanim") return <OphanimField />;
  if (theme === "fallendown") return <FallenField />;
  if (theme === "fireflies") return <FirefliesField />;
  if (theme === "hearth") return <HearthField />;
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


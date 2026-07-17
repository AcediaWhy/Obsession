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
export function HeroField({
  theme,
  frozen = false,
}: {
  theme: Theme;
  frozen?: boolean;
}) {
  // Пока окно скрыто (трей/сворачивание) — размонтируем тяжёлую сцену целиком.
  const renderOn = useRenderActive();
  const hidden = useRenderHidden();
  if (hidden) {
    return (
      <div
        className="absolute inset-0"
        style={{ background: "radial-gradient(130% 130% at 50% 0%, #0b0d12, #050609)" }}
      />
    );
  }

  let scene: ReactNode;
  if (theme === "ophanim") {
    scene = <OphanimField paused={frozen} />;
  } else if (theme === "fallendown") {
    scene = <FallenField paused={frozen} />;
  } else if (theme === "catnap") {
    scene = <CatnapField paused={frozen} />;
  } else if (theme === "midnight") {
    scene = <MidnightField paused={frozen} />;
  } else if (theme === "japan") {
    if (!WEBGL || !renderOn) {
      scene = <RainField2D paused={frozen} />;
    } else {
      scene = (
        <Fallback3D fallback={<RainField2D paused={frozen} />}>
          <Suspense fallback={<RainField2D paused={frozen} />}>
            <RainScene3D paused={frozen} />
          </Suspense>
        </Fallback3D>
      );
    }
  } else {
    scene = <AuroraField paused={frozen} />;
  }

  return (
    <div
      data-scene-frozen={frozen ? "true" : undefined}
      className="pointer-events-none absolute inset-0 overflow-hidden"
    >
      {scene}
    </div>
  );
}

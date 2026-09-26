import { Component, lazy, Suspense, type ReactNode } from "react";
import type { Theme } from "../../store/themeStore";
import { useRenderHidden } from "../render";
import { AuroraField } from "./AuroraField";
import { GoldenMeadowField } from "./GoldenMeadowField";
import { AlchemistField } from "./AlchemistField";
import { FallenField } from "./FallenField";
import { RainFallback } from "./RainFallback";
import { CatnapField } from "./CatnapField";
import { MidnightField } from "./MidnightField";
import { YaniNekoField } from "./YaniNekoField";
import type { ObsessionVisualPhase } from "../obsessionVisualState";

// Гибридная WebGL-сцена Rain грузится только после выбора темы. Suspense и
// любая sync/async ошибка показывают композиционно совпадающий 2D-fallback.
const RainHybridScene = lazy(() => import("./RainHybridScene"));
const YaniCharacterField = lazy(() =>
  import("./YaniCharacterField").then((module) => ({ default: module.YaniCharacterField })),
);

function webglSupported(): boolean {
  try {
    const c = document.createElement("canvas");
    const gl = c.getContext("webgl2") || c.getContext("webgl");
    if (!gl) return false;
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return true;
  } catch {
    return false;
  }
}
let webglAvailable: boolean | undefined;

function webgl2Supported(): boolean {
  try {
    const canvas = document.createElement("canvas");
    const gl = canvas.getContext("webgl2");
    if (!gl) return false;
    gl.getExtension("WEBGL_lose_context")?.loseContext();
    return true;
  } catch {
    return false;
  }
}
let webgl2Available: boolean | undefined;

// Ловим сбои 3D-сцены (драйвер/WebGL/шейдер) и показываем 2D-версию.
class Fallback3D extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch(err: unknown) {
    console.warn("3D-сцена темы упала, откат на 2D:", err);
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
  phase = "idle",
  screen = "overview",
}: {
  theme: Theme;
  frozen?: boolean;
  phase?: ObsessionVisualPhase;
  screen?: string;
}) {
  // Пока окно скрыто (трей/сворачивание) — размонтируем тяжёлую сцену целиком.
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
  if (theme === "goldenmeadow") {
    scene = <GoldenMeadowField paused={frozen} />;
  } else if (theme === "ophanim") {
    scene = <AlchemistField paused={frozen} />;
  } else if (theme === "fallendown") {
    scene = <FallenField paused={frozen} />;
  } else if (theme === "catnap") {
    scene = <CatnapField paused={frozen} />;
  } else if (theme === "midnight") {
    scene = <MidnightField paused={frozen} />;
  } else if (theme === "yanineko") {
    const fallback = <YaniNekoField paused={frozen} phase={phase} screen={screen} />;
    const loading = (
      <div
        aria-hidden="true"
        className="absolute inset-0"
        style={{
          background: "radial-gradient(ellipse at 52% 42%, rgba(100,174,126,.24), transparent 72%), linear-gradient(145deg, #e0eee1, #c8e1cd 43%, #9fc9aa)",
        }}
      />
    );
    scene = (webgl2Available ??= webgl2Supported()) ? (
      <Fallback3D fallback={fallback}>
        <Suspense fallback={loading}>
          <YaniCharacterField
            paused={frozen}
            phase={phase}
            screen={screen}
            storyDensity="story"
            storyDepth="deep"
          />
        </Suspense>
      </Fallback3D>
    ) : fallback;
  } else if (theme === "japan") {
    if (!(webglAvailable ??= webglSupported())) {
      scene = <RainFallback paused={frozen} />;
    } else {
      scene = (
        <Fallback3D fallback={<RainFallback paused={frozen} />}>
          <Suspense fallback={<RainFallback paused={frozen} />}>
            <RainHybridScene paused={frozen} />
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

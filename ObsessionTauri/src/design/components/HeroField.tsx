import { Component, lazy, Suspense, type ReactNode } from "react";
import { useThemeStore } from "../../store/themeStore";
import { AuroraField } from "./AuroraField";
import { OphanimField } from "./OphanimField";
import { FallenField } from "./FallenField";
import { RussiaField } from "./RussiaField";

// 3D-шейдер «Silk» (id japan) грузится лениво (three.js только при выборе темы)
// и только если доступен WebGL; при любой ошибке рендера — откат на AuroraField.
const JapanScene3D = lazy(() => import("./JapanScene3D"));

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
    console.warn("Japan 3D-сцена упала, откат на 2D:", err);
  }
  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

// Диспетчер реактивного фона по выбранной теме.
export function HeroField() {
  const theme = useThemeStore((s) => s.theme);
  if (theme === "ophanim") return <OphanimField />;
  if (theme === "fallendown") return <FallenField />;
  if (theme === "russia") return <RussiaField />;
  if (theme === "japan") {
    if (!WEBGL) return <AuroraField />;
    return (
      <Fallback3D fallback={<AuroraField />}>
        <Suspense fallback={<AuroraField />}>
          <JapanScene3D />
        </Suspense>
      </Fallback3D>
    );
  }
  return <AuroraField />;
}

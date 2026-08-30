import type { Theme } from "../../store/themeStore";
import { AuroraCore } from "./AuroraCore";
import { CatnapCore } from "./CatnapCore";
import { FallenCore } from "./FallenCore";
import { MidnightCore } from "./MidnightCore";
import { YaniCatCore } from "./YaniCatCore";
import { OphanimCore } from "./OphanimCore";
import { RainBenchCore } from "./RainBenchCore";
import { ObsessionChoirCore } from "./ObsessionChoirCore";

const noop = () => {};

// Превью — это тот же renderer, что и у настоящего ядра, а не его схематичная
// подмена. Оно остаётся живым: анимация — часть характера каждой темы. Внешняя
// плитка при этом остаётся единственным интерактивным control.
export function ThemePreview({
  theme,
  size = 104,
  selected = false,
}: {
  theme: Theme;
  size?: number;
  selected?: boolean;
}) {
  const shared = {
    active: selected,
    onClick: noop,
    size,
    paused: false,
  };

  return (
    <div aria-hidden="true" className="relative shrink-0" style={{ width: size, height: size }}>
      {theme === "obsession" && <ObsessionChoirCore {...shared} interactive={false} />}
      {theme === "aurora" && <AuroraCore {...shared} interactive={false} />}
      {theme === "ophanim" && <OphanimCore {...shared} interactive={false} />}
      {theme === "japan" && <RainBenchCore {...shared} interactive={false} />}
      {theme === "fallendown" && <FallenCore {...shared} interactive={false} />}
      {theme === "catnap" && <CatnapCore {...shared} interactive={false} />}
      {theme === "midnight" && <MidnightCore {...shared} interactive={false} />}
      {theme === "yanineko" && <YaniCatCore {...shared} interactive={false} />}
    </div>
  );
}

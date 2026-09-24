import type { Theme } from "../../store/themeStore";
import { AuroraCore } from "./AuroraCore";
import { GoldenMeadowCore } from "./GoldenMeadowCore";
import { CatnapCore } from "./CatnapCore";
import { FallenCore } from "./FallenCore";
import { MidnightCore } from "./MidnightCore";
import { YaniCatCore } from "./YaniCatCore";
import { AlchemistCore } from "./AlchemistCore";
import { RainBenchCore } from "./RainBenchCore";
import { ObsessionChoirCore } from "./ObsessionChoirCore";

const noop = () => {};

// Превью использует компоненты тем с включённой анимацией.
// Нажатия обрабатывает внешняя плитка; само превью неинтерактивно.
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
      {theme === "goldenmeadow" && <GoldenMeadowCore {...shared} interactive={false} />}
      {theme === "aurora" && <AuroraCore {...shared} interactive={false} />}
      {theme === "ophanim" && <AlchemistCore {...shared} interactive={false} />}
      {theme === "japan" && <RainBenchCore {...shared} interactive={false} />}
      {theme === "fallendown" && <FallenCore {...shared} interactive={false} />}
      {theme === "catnap" && <CatnapCore {...shared} interactive={false} />}
      {theme === "midnight" && <MidnightCore {...shared} interactive={false} />}
      {theme === "yanineko" && <YaniCatCore {...shared} interactive={false} />}
    </div>
  );
}

import { useThemeStore } from "../../store/themeStore";
import { AuroraCore } from "./AuroraCore";
import { RainCore } from "./RainCore";
import { OphanimCore } from "./OphanimCore";
import { FallenCore } from "./FallenCore";
import { CatnapCore } from "./CatnapCore";
import { MidnightCore } from "./MidnightCore";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
  // Реактивная телеметрия щита — читает только Ophanim (Великое Око). Остальные
  // ядра игнорируют лишние пропсы; экраны, не знающие про тему, могут их не слать.
  scanning?: boolean;
  alarm?: boolean;
};

// Диспетчер hero-ядра по выбранной теме. Все варианты имеют одинаковый
// интерфейс — экраны просто рендерят <HeroCore/> и не знают про тему.
export function HeroCore(props: Props) {
  const theme = useThemeStore((s) => s.theme);
  if (theme === "ophanim") return <OphanimCore {...props} />;
  if (theme === "fallendown") return <FallenCore {...props} />;
  if (theme === "catnap") return <CatnapCore {...props} />;
  if (theme === "midnight") return <MidnightCore {...props} />;
  // «Rain» (id japan): поверхность воды с расходящейся рябью от капель.
  if (theme === "japan") return <RainCore {...props} />;
  // Aurora — исходное ядро (световые шторы).
  return <AuroraCore {...props} />;
}


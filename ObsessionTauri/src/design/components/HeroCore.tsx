import { useThemeStore } from "../../store/themeStore";
import { AuroraCore } from "./AuroraCore";
import { RainCore } from "./RainCore";
import { OphanimCore } from "./OphanimCore";
import { FallenCore } from "./FallenCore";
import { RussiaCore } from "./RussiaCore";

type Props = {
  active: boolean;
  busy?: boolean;
  onClick: () => void;
  size?: number;
};

// Диспетчер hero-ядра по выбранной теме. Все варианты имеют одинаковый
// интерфейс — экраны просто рендерят <HeroCore/> и не знают про тему.
export function HeroCore(props: Props) {
  const theme = useThemeStore((s) => s.theme);
  if (theme === "ophanim") return <OphanimCore {...props} />;
  if (theme === "fallendown") return <FallenCore {...props} />;
  if (theme === "russia") return <RussiaCore {...props} />;
  // «Rain» (id japan): поверхность воды с расходящейся рябью от капель.
  if (theme === "japan") return <RainCore {...props} />;
  // Aurora — исходное ядро (световые шторы).
  return <AuroraCore {...props} />;
}

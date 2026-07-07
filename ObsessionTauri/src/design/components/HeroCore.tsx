import { useThemeStore } from "../../store/themeStore";
import { AuroraCore } from "./AuroraCore";
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
  // «Silk» (id japan) использует абстрактное ядро Aurora — в тон текучему свету.
  return <AuroraCore {...props} />;
}

import {
  Activity,
  Blend,
  Bot,
  Check,
  ChevronDown,
  CircleAlert,
  CloudRain,
  Contrast,
  Copy,
  Eye,
  Gauge,
  Globe,
  Grid2x2,
  Info,
  Layers,
  type LucideIcon,
  List,
  Minus,
  Palette,
  Pause,
  Play,
  Plus,
  Radar,
  RefreshCw,
  Ruler,
  Send,
  Settings,
  Shield,
  Sparkles,
  Square,
  Terminal,
  Trash,
  TriangleAlert,
  X,
  Zap,
} from "lucide-react";
import type { ComponentType } from "react";

// Иконки лаборатории используют Lucide с размерами 16, 20 и 24 пикселя.
// absoluteStrokeWidth сохраняет толщину обводки 2 экранных пикселя
// при любом из этих размеров; пиксельные элементы самой сцены рисует Canvas.
const SIZES = [16, 20, 24] as const;

export type IconSize = (typeof SIZES)[number];

export type PixelIconProps = {
  size?: number;
  className?: string;
  strokeWidth?: number;
};

/** Прижимает произвольный размер к ближайшему значению шкалы. */
export function snapIconSize(size: number): IconSize {
  return SIZES.reduce<IconSize>(
    (best, candidate) => (Math.abs(candidate - size) < Math.abs(best - size) ? candidate : best),
    SIZES[0],
  );
}

function wrap(Glyph: LucideIcon): ComponentType<PixelIconProps> {
  return function PixelIcon({ size = 20, className = "", strokeWidth = 2 }: PixelIconProps) {
    return (
      <Glyph absoluteStrokeWidth aria-hidden className={className} size={snapIconSize(size)} strokeWidth={strokeWidth} />
    );
  };
}

/**
 * Имена совпадают с общим набором приложения (`Icon.Bolt`, `Icon.Robot`, …), чтобы
 * разметка лабы читалась так же, как боевые экраны, и разница была только в скине.
 */
export const Icon = {
  Bolt: wrap(Zap),
  Robot: wrap(Bot),
  Send: wrap(Send),
  List: wrap(List),
  Layers: wrap(Layers),
  Settings: wrap(Settings),
  Shield: wrap(Shield),
  Refresh: wrap(RefreshCw),
  Copy: wrap(Copy),
  Check: wrap(Check),
  Chevron: wrap(ChevronDown),
  Plus: wrap(Plus),
  Minus: wrap(Minus),
  Trash: wrap(Trash),
  X: wrap(X),
  Info: wrap(Info),
  Alert: wrap(TriangleAlert),
  Danger: wrap(CircleAlert),
  Globe: wrap(Globe),
  Eye: wrap(Eye),
  // Лабораторный набор: инспектор палитры, планов, сетки и часов.
  Palette: wrap(Palette),
  Planes: wrap(Layers),
  Grid: wrap(Grid2x2),
  Ruler: wrap(Ruler),
  Dither: wrap(Blend),
  Contrast: wrap(Contrast),
  Sparkles: wrap(Sparkles),
  Rain: wrap(CloudRain),
  Radar: wrap(Radar),
  Gauge: wrap(Gauge),
  Pulse: wrap(Activity),
  Terminal: wrap(Terminal),
  Play: wrap(Play),
  Pause: wrap(Pause),
  Square: wrap(Square),
} as const;

export type IconName = keyof typeof Icon;

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

// Иконки лабы = lucide-react по его собственной спецификации: канвас 24×24,
// обводка 2px по центру пути, круглые cap/join. Общий src/design/components/icons.tsx
// не трогаем — там свои SVG со strokeWidth 1.8, и они принадлежат боевому приложению.
//
// Почему вектор, а не пиксельные глифы: хром лабы живёт на НАТИВНОМ dpi (доктрина
// двух разрешений), поэтому 2px-обводка Lucide остаётся чёткой, а не борется с
// апскейлом арт-буфера. Пиксельные глифы нужны только внутри мира — их рисует
// рендерер сцены.
//
// Размеры берём из шкалы 16 / 20 / 24 — кратные 4 значения, на которых 2px-штрих
// попадает в пиксельную сетку без полутонов.
//
// absoluteStrokeWidth — сознательное отступление от буквы спеки Lucide. Спека
// требует 2 единицы на канвасе 24×24; при size=16 это дало бы 1.33 экранного
// пикселя, то есть размытый штрих рядом с хрустким артом. absoluteStrokeWidth
// пересчитывает толщину так, чтобы НА ЭКРАНЕ всегда было ровно 2px (в разметке это
// видно как stroke-width="3" при size=16). Геометрия сетки при этом слегка уезжает
// от канонической, но резкость здесь важнее.
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

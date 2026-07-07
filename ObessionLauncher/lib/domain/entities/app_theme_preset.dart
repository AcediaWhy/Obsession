import 'dart:ui';

/// Пресеты визуальной темы Obsession.
///
/// Каждый пресет задаёт не только цвета, но и физику чёрной дыры,
/// интенсивность glow, типографику и общее настроение интерфейса.
///
/// Тёмные пресеты ([obsession], [obsidian], [terminal], [eclipse]) используют
/// космический неоновый стек (чёрная дыра / частицы / гравитационная сетка).
/// Светлые пресеты ([auroraMist], [candyTerminal], [seraphim]) используют
/// светлый пастельный стек ([LightBackgroundConfig]).
///
/// Примечание: этот enum относится к доменной модели настроек, поэтому
/// живёт в domain-слое (presentation зависит от domain, но не наоборот).
enum AppThemePreset {
  /// Классический Obsession: индиго-фиолетовый неон, живой glow,
  /// умеренная сингулярность.
  obsession,

  /// Мрачный и монолитный: тёмно-фиолетовый, почти отсутствующий glow,
  /// матовые поверхности.
  obsidian,

  /// Хакерская эстетика: зелёный терминал на чёрном, моноширинный шрифт,
  /// резкий контраст.
  terminal,

  /// Агрессивная энергия: чёрный + насыщенный оранжево-красный,
  /// ощущение горения и напряжения.
  eclipse,

  /// Aurora Mist: soft premium control center. Молочно-белая база с мягким
  /// вертикальным градиентом, дрейфующие розовые/мятные/голубые/лавандовые
  /// blur-пятна. Светлая тема.
  auroraMist,

  /// Candy Terminal: cute hacker desk. Голубовато-белый фон с микрогридом,
  /// pastel-orbs, мягкие сканлайны. Светлая тема.
  candyTerminal,

  /// Seraphim Desktop: angelic control panel. Молочно-белый фон,
  /// перламутровые панели, розово-голубые ореолы, тонкие вращающиеся кольца,
  /// мягкий bloom и god rays. Светлая тема.
  seraphim,

  /// Пользовательский пресет: значения берутся из [AppSettings].
  custom,
}

/// Режим анимированного фона.
///
/// Тёмные режимы ([blackHole]) рендерят космический стек. Светлые режимы
/// ([auroraDrift], [candyGrid], [seraphimHalo]) рендерятся параметризованным
/// [LightBackground]. [none] — статичный градиент без эффектов.
enum BackgroundMode {
  /// Чёрная дыра + гравитационная сетка + поле частиц (тёмный стек).
  blackHole,

  /// Дрейфующие пастельные blur-блобы с параллаксом (светлый, Aurora Mist).
  auroraDrift,

  /// Микрогрид + pastel-orbs + soft scanlines (светлый, Candy Terminal).
  candyGrid,

  /// Розово-голубые halos + вращающиеся кольца + bloom + god rays (светлый, Seraphim).
  seraphimHalo,

  /// Без анимированного фона (статичная база).
  none,
}

/// Яркость темы — определяет тёмный или светлый визуальный стек.
enum ThemeBrightness { dark, light }

/// Спецификация дрейфующего halo/blob для светлого фона.
class HaloSpec {
  final Color color;
  final double size;
  final Offset startPosition;
  final Offset drift;
  final int durationMs;
  /// Глубина параллакса: 0 = неподвижен, 1 = полностью следует за курсором.
  final double parallaxDepth;

  const HaloSpec({
    required this.color,
    required this.size,
    this.startPosition = Offset.zero,
    this.drift = Offset.zero,
    this.durationMs = 12000,
    this.parallaxDepth = 0.15,
  });
}

/// Спецификация вращающихся орбитальных колец (Seraphim).
class RingSpec {
  /// Количество колец.
  final int count;
  /// Базовый радиус первого кольца (в долях от ширины экрана).
  final double baseRadius;
  /// Шаг радиуса между кольцами.
  final double radiusStep;
  /// Скорость вращения (радиан в секунду).
  final double speed;
  /// Наклон плоскости колец в перспективе (радианы).
  final double tilt;
  final Color color;
  final double thickness;

  const RingSpec({
    this.count = 3,
    this.baseRadius = 0.22,
    this.radiusStep = 0.14,
    this.speed = 0.08,
    this.tilt = 0.55,
    this.color = const Color(0xFFE1D4FF),
    this.thickness = 1.2,
  });
}

/// Спецификация микрогрида (Candy Terminal).
class GridSpec {
  final double cellSize;
  final Color color;
  /// Радиус подсветки вокруг курсора (px).
  final double cursorGlowRadius;
  final double cursorGlowIntensity;

  const GridSpec({
    this.cellSize = 36,
    this.color = const Color(0xFFC7E9FF),
    this.cursorGlowRadius = 180,
    this.cursorGlowIntensity = 0.5,
  });
}

/// Спецификация редких мерцающих sparkles.
class SparkleSpec {
  final Color color;
  final int count;
  final double maxSize;
  /// Частота мерцания (период в мс).
  final int twinklePeriodMs;

  const SparkleSpec({
    this.color = const Color(0xFFFFFFFF),
    this.count = 24,
    this.maxSize = 1.8,
    this.twinklePeriodMs = 4000,
  });
}

/// Конфигурация параметризованного светлого фона.
/// Каждый светлый пресет отдаёт свой [LightBackgroundConfig] через
/// [AppThemeData.lightBackground].
class LightBackgroundConfig {
  /// Вертикальный градиент базы (сверху вниз).
  final List<Color> baseGradient;
  /// Дрейфующие halo/blob с параллаксом.
  final List<HaloSpec> halos;
  /// Вращающиеся кольца (null = нет).
  final RingSpec? rings;
  /// Микрогрид (null = нет).
  final GridSpec? grid;
  /// Редкие sparkles (null = нет).
  final SparkleSpec? sparkles;
  /// Интенсивность общего soft bloom.
  final double bloomIntensity;
  /// Мягкие god rays (только Seraphim).
  final bool godRays;
  /// Цвет cursor-halo (мягкое свечение под курсором). null = нет halo.
  final Color? cursorHaloColor;
  /// Радиус cursor-halo (px).
  final double cursorHaloRadius;

  const LightBackgroundConfig({
    required this.baseGradient,
    required this.halos,
    this.rings,
    this.grid,
    this.sparkles,
    this.bloomIntensity = 0.3,
    this.godRays = false,
    this.cursorHaloColor,
    this.cursorHaloRadius = 220,
  });
}

/// Расширенные параметры темы, которые определяют не только цвета,
/// но и поведение визуальных эффектов.
class AppThemeData {
  final Color accentColor;
  final Color secondaryAccent;
  final double glowIntensity;
  final double blackHoleRadius;
  final double blackHoleDiskBrightness;
  final double blackHoleLensIntensity;
  final double particleDensity;
  final String uiFont;
  final String monoFont;
  final double borderRadius;
  final bool sharpAngles;

  // --- Светлый стек ---

  /// Яркость темы. Тёмные пресеты = [ThemeBrightness.dark], светлые = [ThemeBrightness.light].
  final ThemeBrightness brightness;

  /// Дефолтный режим фона для пресета.
  final BackgroundMode defaultBackgroundMode;

  /// Текст: основной цвет.
  final Color textPrimary;
  /// Текст: вторичный.
  final Color textSecondary;
  /// Текст: приглушённый.
  final Color textMuted;

  /// Цвет полупрозрачной карточки (для светлых тем — белый с alpha).
  final Color cardColor;
  /// Непрозрачность карточки в покое.
  final double cardOpacity;
  /// Непрозрачность карточки на hover.
  final double cardOpacityHover;
  /// Цвет границы карточки.
  final Color cardBorderColor;
  /// Цвет edge-bloom на hover (пастельная подсветка края).
  final Color edgeBloomColor;

  /// Шрифт заголовков (для светлых тем — Nunito).
  final String headingFont;
  /// Шрифт основного текста (для светлых тем — Plus Jakarta Sans).
  final String bodyFont;

  /// Конфиг светлого фона (null для тёмных пресетов).
  final LightBackgroundConfig? lightBackground;

  /// Интенсивность перламутрового перелива на акцентных панелях (0..1).
  /// 0 = обычный glass, 1 = полный iridescence. Только Seraphim > 0.
  final double iridescenceIntensity;

  /// Цвета перламутрового перелива (thin-film tints).
  final List<Color> iridescenceTints;

  const AppThemeData({
    required this.accentColor,
    required this.secondaryAccent,
    required this.glowIntensity,
    required this.blackHoleRadius,
    required this.blackHoleDiskBrightness,
    required this.blackHoleLensIntensity,
    required this.particleDensity,
    required this.uiFont,
    required this.monoFont,
    required this.borderRadius,
    required this.sharpAngles,
    this.brightness = ThemeBrightness.dark,
    this.defaultBackgroundMode = BackgroundMode.blackHole,
    this.textPrimary = const Color(0xFFF8FAFC),
    this.textSecondary = const Color(0xFF94A3B8),
    this.textMuted = const Color(0xFF7C8AA1),
    this.cardColor = const Color(0xFFFFFFFF),
    this.cardOpacity = 0.04,
    this.cardOpacityHover = 0.06,
    this.cardBorderColor = const Color(0xFFFFFFFF),
    this.edgeBloomColor = const Color(0xFF6366F1),
    this.headingFont = 'Inter',
    this.bodyFont = 'Inter',
    this.lightBackground,
    this.iridescenceIntensity = 0.0,
    this.iridescenceTints = const [],
  });

  /// True, если пресет светлый (использует светлый визуальный стек).
  bool get isLight => brightness == ThemeBrightness.light;
}

extension AppThemePresetData on AppThemePreset {
  /// Возвращает параметры пресета по умолчанию.
  AppThemeData get data {
    switch (this) {
      case AppThemePreset.obsession:
        return const AppThemeData(
          accentColor: Color(0xFF6366F1),
          secondaryAccent: Color(0xFF8B5CF6),
          glowIntensity: 0.6,
          blackHoleRadius: 0.18,
          blackHoleDiskBrightness: 1.0,
          blackHoleLensIntensity: 0.6,
          particleDensity: 0.5,
          uiFont: 'Inter',
          monoFont: 'JetBrains Mono',
          borderRadius: 12.0,
          sharpAngles: false,
        );
      case AppThemePreset.obsidian:
        return const AppThemeData(
          accentColor: Color(0xFF7C3AED),
          secondaryAccent: Color(0xFF475569),
          glowIntensity: 0.25,
          blackHoleRadius: 0.15,
          blackHoleDiskBrightness: 0.5,
          blackHoleLensIntensity: 0.4,
          particleDensity: 0.25,
          uiFont: 'Inter',
          monoFont: 'JetBrains Mono',
          borderRadius: 8.0,
          sharpAngles: false,
        );
      case AppThemePreset.terminal:
        return const AppThemeData(
          accentColor: Color(0xFF00FF41),
          secondaryAccent: Color(0xFF00CC33),
          glowIntensity: 0.5,
          blackHoleRadius: 0.12,
          blackHoleDiskBrightness: 0.3,
          blackHoleLensIntensity: 0.25,
          particleDensity: 0.15,
          uiFont: 'Inter',
          monoFont: 'JetBrains Mono',
          borderRadius: 8.0,
          sharpAngles: false,
        );
      case AppThemePreset.eclipse:
        return const AppThemeData(
          accentColor: Color(0xFFF97316),
          secondaryAccent: Color(0xFFDC2626),
          glowIntensity: 0.75,
          blackHoleRadius: 0.2,
          blackHoleDiskBrightness: 1.25,
          blackHoleLensIntensity: 0.7,
          particleDensity: 0.6,
          uiFont: 'Inter',
          monoFont: 'JetBrains Mono',
          borderRadius: 6.0,
          sharpAngles: false,
        );
      case AppThemePreset.auroraMist:
        return const AppThemeData(
          accentColor: Color(0xFFF7B6D2), // Pastel Pink
          secondaryAccent: Color(0xFFBFE7FF), // Soft Blue
          glowIntensity: 0.35,
          blackHoleRadius: 0.18,
          blackHoleDiskBrightness: 1.0,
          blackHoleLensIntensity: 0.6,
          particleDensity: 0.4,
          uiFont: 'Plus Jakarta Sans',
          monoFont: 'JetBrains Mono',
          borderRadius: 22.0,
          sharpAngles: false,
          brightness: ThemeBrightness.light,
          defaultBackgroundMode: BackgroundMode.auroraDrift,
          textPrimary: Color(0xFF5C6073),
          textSecondary: Color(0xFF8A8FA3),
          textMuted: Color(0xFFA8ADBF),
          cardColor: Color(0xFFFFFFFF),
          cardOpacity: 0.55,
          cardOpacityHover: 0.72,
          cardBorderColor: Color(0xFFFFFFFF),
          edgeBloomColor: Color(0xFFF7B6D2),
          headingFont: 'Nunito',
          bodyFont: 'Plus Jakarta Sans',
          lightBackground: LightBackgroundConfig(
            baseGradient: [
              Color(0xFFFFF8FB),
              Color(0xFFF6FBFF),
              Color(0xFFF7F4FF),
            ],
            halos: [
              HaloSpec(
                color: Color(0xFFF7B6D2), // pink
                size: 420,
                startPosition: Offset(-80, -120),
                drift: Offset(90, 70),
                durationMs: 14000,
                parallaxDepth: 0.18,
              ),
              HaloSpec(
                color: Color(0xFFCFF7E8), // mint
                size: 380,
                startPosition: Offset(280, 120),
                drift: Offset(-70, 60),
                durationMs: 16000,
                parallaxDepth: 0.12,
              ),
              HaloSpec(
                color: Color(0xFFBFE7FF), // blue
                size: 460,
                startPosition: Offset(120, 360),
                drift: Offset(60, -80),
                durationMs: 18000,
                parallaxDepth: 0.22,
              ),
              HaloSpec(
                color: Color(0xFFD9C6FF), // lavender
                size: 340,
                startPosition: Offset(-60, 300),
                drift: Offset(80, -50),
                durationMs: 15000,
                parallaxDepth: 0.15,
              ),
            ],
            sparkles: SparkleSpec(
              color: Color(0xFFFFFFFF),
              count: 18,
              maxSize: 1.6,
              twinklePeriodMs: 5000,
            ),
            bloomIntensity: 0.25,
            cursorHaloColor: Color(0xFFF7B6D2),
            cursorHaloRadius: 240,
          ),
        );
      case AppThemePreset.candyTerminal:
        return const AppThemeData(
          accentColor: Color(0xFFF5BDD6), // Pink (только для primary CTA)
          secondaryAccent: Color(0xFFC9F3DD), // Mint
          glowIntensity: 0.4,
          blackHoleRadius: 0.18,
          blackHoleDiskBrightness: 1.0,
          blackHoleLensIntensity: 0.6,
          particleDensity: 0.4,
          uiFont: 'Plus Jakarta Sans',
          monoFont: 'JetBrains Mono',
          borderRadius: 14.0,
          sharpAngles: false,
          brightness: ThemeBrightness.light,
          defaultBackgroundMode: BackgroundMode.candyGrid,
          textPrimary: Color(0xFF566074),
          textSecondary: Color(0xFF838DA1),
          textMuted: Color(0xFFA3ADBF),
          cardColor: Color(0xFFFAFCFF),
          cardOpacity: 0.62,
          cardOpacityHover: 0.78,
          cardBorderColor: Color(0xFFC7E9FF),
          edgeBloomColor: Color(0xFFC7E9FF),
          headingFont: 'Nunito',
          bodyFont: 'Plus Jakarta Sans',
          lightBackground: LightBackgroundConfig(
            baseGradient: [
              Color(0xFFF7FBFF),
              Color(0xFFEFF6FF),
            ],
            halos: [
              HaloSpec(
                color: Color(0xFFC7E9FF), // baby blue
                size: 400,
                startPosition: Offset(-60, -100),
                drift: Offset(80, 60),
                durationMs: 13000,
                parallaxDepth: 0.16,
              ),
              HaloSpec(
                color: Color(0xFFD7CCFF), // lavender
                size: 320,
                startPosition: Offset(260, 200),
                drift: Offset(-60, -40),
                durationMs: 15000,
                parallaxDepth: 0.13,
              ),
              HaloSpec(
                color: Color(0xFFC9F3DD), // mint
                size: 300,
                startPosition: Offset(80, 380),
                drift: Offset(50, -60),
                durationMs: 17000,
                parallaxDepth: 0.2,
              ),
            ],
            grid: GridSpec(
              cellSize: 34,
              color: Color(0xFFC7E9FF),
              cursorGlowRadius: 200,
              cursorGlowIntensity: 0.55,
            ),
            bloomIntensity: 0.2,
            cursorHaloColor: Color(0xFFC7E9FF),
            cursorHaloRadius: 200,
          ),
        );
      case AppThemePreset.seraphim:
        return const AppThemeData(
          accentColor: Color(0xFFF8C4D8), // Blush Pink
          secondaryAccent: Color(0xFFD8EEFF), // Mist Blue
          glowIntensity: 0.45,
          blackHoleRadius: 0.18,
          blackHoleDiskBrightness: 1.0,
          blackHoleLensIntensity: 0.6,
          particleDensity: 0.35,
          uiFont: 'Plus Jakarta Sans',
          monoFont: 'JetBrains Mono',
          borderRadius: 24.0,
          sharpAngles: false,
          brightness: ThemeBrightness.light,
          defaultBackgroundMode: BackgroundMode.seraphimHalo,
          textPrimary: Color(0xFF62667A),
          textSecondary: Color(0xFF8B8FA3),
          textMuted: Color(0xFFAAAEBF),
          cardColor: Color(0xFFFFFCFF),
          cardOpacity: 0.6,
          cardOpacityHover: 0.78,
          cardBorderColor: Color(0xFFFFFFFF),
          edgeBloomColor: Color(0xFFF8C4D8),
          headingFont: 'Nunito',
          bodyFont: 'Plus Jakarta Sans',
          iridescenceIntensity: 0.7,
          iridescenceTints: [
            Color(0xFFFFFCFF), // cloud white
            Color(0xFFF8C4D8), // blush pink
            Color(0xFFD8EEFF), // mist blue
            Color(0xFFE1D4FF), // dream lavender
            Color(0xFFDDF9EE), // mint glow
          ],
          lightBackground: LightBackgroundConfig(
            baseGradient: [
              Color(0xFFFFFCFF),
              Color(0xFFFFF6FA),
              Color(0xFFF6FBFF),
            ],
            halos: [
              HaloSpec(
                color: Color(0xFFF8C4D8), // blush pink halo
                size: 480,
                startPosition: Offset(-100, -140),
                drift: Offset(90, 80),
                durationMs: 16000,
                parallaxDepth: 0.2,
              ),
              HaloSpec(
                color: Color(0xFFD8EEFF), // mist blue halo
                size: 520,
                startPosition: Offset(220, 80),
                drift: Offset(-80, 60),
                durationMs: 19000,
                parallaxDepth: 0.16,
              ),
              HaloSpec(
                color: Color(0xFFE1D4FF), // dream lavender halo
                size: 420,
                startPosition: Offset(60, 360),
                drift: Offset(70, -70),
                durationMs: 17000,
                parallaxDepth: 0.24,
              ),
              HaloSpec(
                color: Color(0xFFDDF9EE), // mint glow halo
                size: 360,
                startPosition: Offset(-80, 280),
                drift: Offset(60, -40),
                durationMs: 15000,
                parallaxDepth: 0.14,
              ),
            ],
            rings: RingSpec(
              count: 3,
              baseRadius: 0.24,
              radiusStep: 0.16,
              speed: 0.06,
              tilt: 0.6,
              color: Color(0xFFE1D4FF),
              thickness: 1.3,
            ),
            sparkles: SparkleSpec(
              color: Color(0xFFFFFFFF),
              count: 26,
              maxSize: 2.0,
              twinklePeriodMs: 4500,
            ),
            bloomIntensity: 0.4,
            godRays: true,
            cursorHaloColor: Color(0xFFF8C4D8),
            cursorHaloRadius: 260,
          ),
        );
      case AppThemePreset.custom:
        // Для custom значения реально берутся из AppSettings — см. AppTheme.resolvePreset().
        // Этот fallback здесь только чтобы enum оставался исчерпывающим.
        return const AppThemeData(
          accentColor: Color(0xFF6366F1),
          secondaryAccent: Color(0xFF8B5CF6),
          glowIntensity: 0.6,
          blackHoleRadius: 0.18,
          blackHoleDiskBrightness: 1.0,
          blackHoleLensIntensity: 0.6,
          particleDensity: 0.5,
          uiFont: 'Inter',
          monoFont: 'JetBrains Mono',
          borderRadius: 12.0,
          sharpAngles: false,
        );
    }
  }

  /// Отображаемое имя пресета.
  String get label {
    switch (this) {
      case AppThemePreset.obsession:
        return 'Obsession';
      case AppThemePreset.obsidian:
        return 'Obsidian';
      case AppThemePreset.terminal:
        return 'Terminal';
      case AppThemePreset.eclipse:
        return 'Eclipse';
      case AppThemePreset.auroraMist:
        return 'Aurora Mist';
      case AppThemePreset.candyTerminal:
        return 'Candy Terminal';
      case AppThemePreset.seraphim:
        return 'Seraphim';
      case AppThemePreset.custom:
        return 'Custom';
    }
  }

  /// True, если пресет светлый.
  bool get isLight => data.isLight;
}

extension BackgroundModeData on BackgroundMode {
  /// Отображаемое имя режима фона.
  String get label {
    switch (this) {
      case BackgroundMode.blackHole:
        return 'Black hole';
      case BackgroundMode.auroraDrift:
        return 'Aurora drift';
      case BackgroundMode.candyGrid:
        return 'Candy grid';
      case BackgroundMode.seraphimHalo:
        return 'Seraphim halo';
      case BackgroundMode.none:
        return 'None';
    }
  }

  /// True, если режим относится к светлому стеку.
  bool get isLight =>
      this == BackgroundMode.auroraDrift ||
      this == BackgroundMode.candyGrid ||
      this == BackgroundMode.seraphimHalo;
}

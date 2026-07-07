// ignore: unused_import
import 'package:intl/intl.dart' as intl;
import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Russian (`ru`).
class AppLocalizationsRu extends AppLocalizations {
  AppLocalizationsRu([String locale = 'ru']) : super(locale);

  @override
  String get appName => 'Obsession';

  @override
  String get dpiTitle => 'DPI обход';

  @override
  String get dpiSubtitle => 'Управление обходом блокировок через WinDivert.';

  @override
  String get categories => 'Категории';

  @override
  String get canSelectMultiple => 'Можно выбрать несколько';

  @override
  String get config => 'Конфиг';

  @override
  String get autoSelect => 'Автоподбор';

  @override
  String get test => 'Проверить';

  @override
  String testingConfig(int current, int total, String config) {
    return 'Проверка $current/$total: $config';
  }

  @override
  String testingCategory(int current, int total, String config) {
    return 'Автоподбор категории $current/$total: $config';
  }

  @override
  String get start => 'START';

  @override
  String get stop => 'Остановить';

  @override
  String get active => 'АКТИВЕН';

  @override
  String get inactive => 'НЕ АКТИВЕН';

  @override
  String get activeShort => 'Активен';

  @override
  String get inactiveShort => 'Остановлен';

  @override
  String get protectionActive => 'Защита активна';

  @override
  String get protectionStopped => 'Остановлен';

  @override
  String get working => 'Рабочий';

  @override
  String get notWorking => 'Нерабочий';

  @override
  String get settings => 'Настройки';

  @override
  String get settingsSubtitle =>
      'Кастомизация внешнего вида и поведения приложения.';

  @override
  String get appearance => 'Внешний вид';

  @override
  String get themePreset => 'Пресет темы';

  @override
  String get themeGroupDark => 'Тёмные';

  @override
  String get themeGroupLight => 'Светлые';

  @override
  String get backgroundMode => 'Режим фона';

  @override
  String get backgroundModeAuto => 'Авто (из темы)';

  @override
  String get accentColor => 'Акцентный цвет';

  @override
  String get glowIntensity => 'Интенсивность свечения';

  @override
  String get blurStrength => 'Сила размытия (glass)';

  @override
  String get backgroundSpeed => 'Скорость фоновых анимаций';

  @override
  String get particleDensity => 'Плотность частиц';

  @override
  String get animationsEnabled => 'Анимации включены';

  @override
  String get blackHoleEnabled => 'Чёрная дыра на фоне';

  @override
  String get blackHole => 'Чёрная дыра';

  @override
  String get blackHoleRadius => 'Размер чёрной дыры';

  @override
  String get blackHoleDiskBrightness => 'Яркость аккреционного диска';

  @override
  String get blackHoleLensIntensity => 'Сила гравитационного линзирования';

  @override
  String get behavior => 'Поведение';

  @override
  String get minimizeToTray => 'Сворачивать в трей при закрытии';

  @override
  String get autostart => 'Автозапуск при старте системы';

  @override
  String get resetSettings => 'Сбросить настройки по умолчанию';

  @override
  String get resetConfirmTitle => 'Сбросить настройки?';

  @override
  String get resetConfirmMessage =>
      'Все параметры кастомизации вернутся к значениям по умолчанию.';

  @override
  String get cancel => 'Отмена';

  @override
  String get reset => 'Сбросить';

  @override
  String get yes => 'Да';

  @override
  String get no => 'Нет';

  @override
  String get minimize => 'Свернуть';

  @override
  String get maximize => 'Развернуть';

  @override
  String get restore => 'Восстановить';

  @override
  String get close => 'Закрыть';

  @override
  String get language => 'Язык';

  @override
  String get russian => 'Русский';

  @override
  String get english => 'English';

  @override
  String get liveLog => 'Live log';

  @override
  String get logTitle => 'ЖУРНАЛ';

  @override
  String get waitingToStart => 'Ожидание запуска...';

  @override
  String get statistics => 'Статистика';

  @override
  String get status => 'Статус';

  @override
  String get dpiBypass => 'DPI обход';

  @override
  String get telegramProxy => 'Telegram прокси';

  @override
  String get hosts => 'Hosts';

  @override
  String get activeProcesses => 'Активных процессов';

  @override
  String activeProcessesCount(int count) {
    return '$count процессов';
  }

  @override
  String get selectedConfigs => 'Выбрано конфигов';

  @override
  String get version => 'Версия';

  @override
  String versionWithValue(String version) {
    return 'Версия: $version';
  }

  @override
  String get uptime => 'Время работы';

  @override
  String get noActiveProcesses => 'Нет активных процессов';

  @override
  String get selectedConfigsTitle => 'ВЫБРАННЫЕ КОНФИГИ';

  @override
  String get restart => 'Перезапустить';

  @override
  String get configFolder => 'Папка конфигов';

  @override
  String get singularityStatus => 'SINGULARITY STATUS';

  @override
  String get installed => 'Установлен';

  @override
  String get notInstalled => 'Не установлен';

  @override
  String get logEmpty => 'Лог пуст';

  @override
  String get menu => 'МЕНЮ';

  @override
  String get aiTitle => 'ИИ обход';

  @override
  String get aiSubtitle =>
      'Разблокировка ChatGPT, Claude, Gemini через hosts-файл.';

  @override
  String get provider => 'ПРОВАЙДЕР';

  @override
  String get install => 'Установить';

  @override
  String get update => 'Обновить';

  @override
  String get uninstall => 'Удалить';

  @override
  String get check => 'Проверить';

  @override
  String get installedUpToDate => 'Установлено, актуально';

  @override
  String get installedOutdated => 'Установлено, но устарело';

  @override
  String get aiNotInstalled => 'Не установлено';

  @override
  String get uninstallAiTitle => 'Удалить ИИ-обход?';

  @override
  String get uninstallAiMessage =>
      'hosts-файл будет восстановлен к значению по умолчанию.';

  @override
  String get telegramTitle => 'Telegram обход';

  @override
  String get telegramSubtitle =>
      'MTProto WebSocket-мост для разблокировки Telegram Desktop.';

  @override
  String get tgProxyNotFoundTitle => 'TgWsProxy.exe не найден';

  @override
  String get tgProxyNotFoundMessage =>
      'Положите TgWsProxy_windows.exe в папку bin/ и перезапустите приложение.';

  @override
  String get proxySettings => 'НАСТРОЙКИ ПРОКСИ';

  @override
  String get port => 'Порт';

  @override
  String get fakeTlsDomainOptional => 'Fake TLS домен (опц.)';

  @override
  String get notSet => 'не задан';

  @override
  String get connectionLink => 'ССЫЛКА ПОДКЛЮЧЕНИЯ';

  @override
  String get openInTelegram => 'Открыть в Telegram';

  @override
  String get copy => 'Копировать';

  @override
  String get instructions => 'ИНСТРУКЦИЯ';

  @override
  String get telegramInstructions =>
      '1. Нажмите START для запуска прокси\n2. Нажмите «Открыть в Telegram» или скопируйте ссылку\n3. В Telegram: Настройки → Продвинутые → Тип подключения → Прокси';

  @override
  String get listsTitle => 'Редактор списков';

  @override
  String get listsSubtitle =>
      'Редактирование доменов и IP для обхода блокировок.';

  @override
  String get importListDialogTitle => 'Импорт списка';

  @override
  String get exportListDialogTitle => 'Экспорт списка';

  @override
  String get selectListHint => 'Выберите список';

  @override
  String entriesCount(int count) {
    return '$count записей';
  }

  @override
  String get domain => 'Домен';

  @override
  String get ip => 'IP';

  @override
  String get comment => 'Комментарий';

  @override
  String get save => 'Сохранить';

  @override
  String get clearListTitle => 'Очистить список?';

  @override
  String get clearListMessage =>
      'Все записи будут удалены. Это действие нельзя отменить.';

  @override
  String get clear => 'Очистить';

  @override
  String get removeDuplicates => 'Удалить дубликаты';

  @override
  String get listEntryHint => 'Домен или IP';

  @override
  String get commentHint => 'Комментарий';

  @override
  String get profilesTitle => 'Профили';

  @override
  String get profilesSubtitle => 'Сохранённые наборы настроек.';

  @override
  String get profile => 'Профиль';

  @override
  String get newProfile => 'Новый профиль';

  @override
  String get profileName => 'Название профиля';

  @override
  String get create => 'Создать';

  @override
  String get delete => 'Удалить';

  @override
  String get deleteProfileTitle => 'Удалить профиль?';

  @override
  String deleteProfileMessage(String name) {
    return 'Профиль «$name» будет удалён без возможности восстановления.';
  }

  @override
  String get activate => 'Активировать';

  @override
  String profileCategoriesWithProvider(int count, String provider) {
    return '$count категорий · $provider';
  }

  @override
  String get noProfiles => 'Нет сохранённых профилей';

  @override
  String get copyLink => 'Скопировать ссылку';

  @override
  String get proxyPort => 'Порт прокси';

  @override
  String portWithValue(int port) {
    return 'Порт $port';
  }

  @override
  String get fakeTlsDomain => 'Fake TLS домен';

  @override
  String get updateAvailable => 'Доступно обновление';

  @override
  String versionDate(String date) {
    return 'Опубликовано: $date';
  }

  @override
  String get noDescription => 'Нет описания';

  @override
  String get downloadingUpdate => 'Загрузка обновления...';

  @override
  String get download => 'Скачать';

  @override
  String get later => 'Позже';

  @override
  String get folder => 'Папка';

  @override
  String get runInstaller => 'Запустить установщик';

  @override
  String get fileHash => 'SHA256';

  @override
  String get hashHint =>
      'Файл загружен. Сверьте SHA256 с релизом на GitHub перед запуском.';

  @override
  String errorWithMessage(String error) {
    return 'Ошибка: $error';
  }

  @override
  String get onboardingTitle => 'Добро пожаловать в Obsession';

  @override
  String get onboardingSubtitle =>
      'Обход DPI и блокировок с космическим интерфейсом. Нажмите ниже, чтобы начать.';

  @override
  String get onboardingContinue => 'Продолжить';

  @override
  String get onboardingIntro =>
      'Космический лаунчер для обхода блокировок. DPI, ИИ и Telegram — всё в одном окне.';

  @override
  String get onboardingDpi =>
      'Автоматический подбор конфигураций WinDivert для Discord, YouTube, игр и других сервисов.';

  @override
  String get onboardingAi =>
      'Разблокировка ChatGPT, Claude и Gemini через обновление hosts-файла.';

  @override
  String get onboardingTelegram =>
      'Локальный MTProto WebSocket-прокси для разблокировки Telegram Desktop.';

  @override
  String get onboardingProfiles =>
      'Сохраняйте настройки и переключайтесь между ними одним кликом.';

  @override
  String get onboardingStyleTitle => 'Настрой свой стиль';

  @override
  String get onboardingStyle =>
      'Акцентный цвет, свечение, чёрная дыра и анимации — под твоё настроение.';

  @override
  String get skip => 'Пропустить';

  @override
  String get next => 'Далее';

  @override
  String get getStarted => 'Начать';

  @override
  String get open => 'Открыть';

  @override
  String get availability => 'Доступность';

  @override
  String latencyMs(int ms) {
    return '$ms мс';
  }

  @override
  String get unreachable => 'недоступен';

  @override
  String get checking => 'проверка...';

  @override
  String get emergencyStop => 'Экстренный стоп';

  @override
  String get emergencyStopTitle => 'Остановить всё?';

  @override
  String get emergencyStopMessage =>
      'Будут остановлены DPI, Telegram-прокси и удалены hosts-правила. Продолжить?';

  @override
  String get saveLogToFile => 'Сохранить лог в файл';

  @override
  String logSavedTo(String path) {
    return 'Лог сохранён: $path';
  }

  @override
  String get logSaveFailed => 'Не удалось сохранить лог';

  @override
  String get autoDetected => 'Авто-обнаруженные';

  @override
  String get autoDetectedSubtitle =>
      'Домены, которые Zapret сам определил как заблокированные';

  @override
  String get noAutoDetected =>
      'Пока ничего не обнаружено. Список формируется автоматически при использовании обхода.';

  @override
  String get clearAutoDetected => 'Очистить списки';

  @override
  String get autoDetectedCleared => 'Списки авто-обнаружения очищены';

  @override
  String domainsCount(int count) {
    return '$count доменов';
  }

  @override
  String get orphanDetectedTitle => 'Обнаружены зависшие процессы';

  @override
  String orphanDetectedMessage(int count) {
    return 'Найдено $count процесс(ов) winws.exe от предыдущего запуска (возможно, после краша). Они могут мешать запуску обхода. Остановить их?\n\nВнимание: будут остановлены ВСЕ процессы winws.exe в системе, включая запущенные другими DPI-инструментами.';
  }

  @override
  String get orphanDetectedClear => 'Остановить';

  @override
  String get orphanDetectedIgnore => 'Игнорировать';

  @override
  String get orphanDetectedNone => 'Зависших процессов не обнаружено.';

  @override
  String get orphanDetectedCleared => 'Зависшие процессы остановлены.';

  @override
  String get orphanDetectedCleanFailed =>
      'Не удалось остановить зависшие процессы.';
}

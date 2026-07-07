import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/intl.dart' as intl;

import 'app_localizations_en.dart';
import 'app_localizations_ru.dart';

// ignore_for_file: type=lint

/// Callers can lookup localized strings with an instance of AppLocalizations
/// returned by `AppLocalizations.of(context)`.
///
/// Applications need to include `AppLocalizations.delegate()` in their app's
/// `localizationDelegates` list, and the locales they support in the app's
/// `supportedLocales` list. For example:
///
/// ```dart
/// import 'l10n/app_localizations.dart';
///
/// return MaterialApp(
///   localizationsDelegates: AppLocalizations.localizationsDelegates,
///   supportedLocales: AppLocalizations.supportedLocales,
///   home: MyApplicationHome(),
/// );
/// ```
///
/// ## Update pubspec.yaml
///
/// Please make sure to update your pubspec.yaml to include the following
/// packages:
///
/// ```yaml
/// dependencies:
///   # Internationalization support.
///   flutter_localizations:
///     sdk: flutter
///   intl: any # Use the pinned version from flutter_localizations
///
///   # Rest of dependencies
/// ```
///
/// ## iOS Applications
///
/// iOS applications define key application metadata, including supported
/// locales, in an Info.plist file that is built into the application bundle.
/// To configure the locales supported by your app, you’ll need to edit this
/// file.
///
/// First, open your project’s ios/Runner.xcworkspace Xcode workspace file.
/// Then, in the Project Navigator, open the Info.plist file under the Runner
/// project’s Runner folder.
///
/// Next, select the Information Property List item, select Add Item from the
/// Editor menu, then select Localizations from the pop-up menu.
///
/// Select and expand the newly-created Localizations item then, for each
/// locale your application supports, add a new item and select the locale
/// you wish to add from the pop-up menu in the Value field. This list should
/// be consistent with the languages listed in the AppLocalizations.supportedLocales
/// property.
abstract class AppLocalizations {
  AppLocalizations(String locale)
    : localeName = intl.Intl.canonicalizedLocale(locale.toString());

  final String localeName;

  static AppLocalizations of(BuildContext context) {
    return Localizations.of<AppLocalizations>(context, AppLocalizations)!;
  }

  static const LocalizationsDelegate<AppLocalizations> delegate =
      _AppLocalizationsDelegate();

  /// A list of this localizations delegate along with the default localizations
  /// delegates.
  ///
  /// Returns a list of localizations delegates containing this delegate along with
  /// GlobalMaterialLocalizations.delegate, GlobalCupertinoLocalizations.delegate,
  /// and GlobalWidgetsLocalizations.delegate.
  ///
  /// Additional delegates can be added by appending to this list in
  /// MaterialApp. This list does not have to be used at all if a custom list
  /// of delegates is preferred or required.
  static const List<LocalizationsDelegate<dynamic>> localizationsDelegates =
      <LocalizationsDelegate<dynamic>>[
        delegate,
        GlobalMaterialLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
      ];

  /// A list of this localizations delegate's supported locales.
  static const List<Locale> supportedLocales = <Locale>[
    Locale('en'),
    Locale('ru'),
  ];

  /// No description provided for @appName.
  ///
  /// In ru, this message translates to:
  /// **'Obsession'**
  String get appName;

  /// No description provided for @dpiTitle.
  ///
  /// In ru, this message translates to:
  /// **'DPI обход'**
  String get dpiTitle;

  /// No description provided for @dpiSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Управление обходом блокировок через WinDivert.'**
  String get dpiSubtitle;

  /// No description provided for @categories.
  ///
  /// In ru, this message translates to:
  /// **'Категории'**
  String get categories;

  /// No description provided for @canSelectMultiple.
  ///
  /// In ru, this message translates to:
  /// **'Можно выбрать несколько'**
  String get canSelectMultiple;

  /// No description provided for @config.
  ///
  /// In ru, this message translates to:
  /// **'Конфиг'**
  String get config;

  /// No description provided for @autoSelect.
  ///
  /// In ru, this message translates to:
  /// **'Автоподбор'**
  String get autoSelect;

  /// No description provided for @test.
  ///
  /// In ru, this message translates to:
  /// **'Проверить'**
  String get test;

  /// No description provided for @testingConfig.
  ///
  /// In ru, this message translates to:
  /// **'Проверка {current}/{total}: {config}'**
  String testingConfig(int current, int total, String config);

  /// No description provided for @testingCategory.
  ///
  /// In ru, this message translates to:
  /// **'Автоподбор категории {current}/{total}: {config}'**
  String testingCategory(int current, int total, String config);

  /// No description provided for @start.
  ///
  /// In ru, this message translates to:
  /// **'START'**
  String get start;

  /// No description provided for @stop.
  ///
  /// In ru, this message translates to:
  /// **'Остановить'**
  String get stop;

  /// No description provided for @active.
  ///
  /// In ru, this message translates to:
  /// **'АКТИВЕН'**
  String get active;

  /// No description provided for @inactive.
  ///
  /// In ru, this message translates to:
  /// **'НЕ АКТИВЕН'**
  String get inactive;

  /// No description provided for @activeShort.
  ///
  /// In ru, this message translates to:
  /// **'Активен'**
  String get activeShort;

  /// No description provided for @inactiveShort.
  ///
  /// In ru, this message translates to:
  /// **'Остановлен'**
  String get inactiveShort;

  /// No description provided for @protectionActive.
  ///
  /// In ru, this message translates to:
  /// **'Защита активна'**
  String get protectionActive;

  /// No description provided for @protectionStopped.
  ///
  /// In ru, this message translates to:
  /// **'Остановлен'**
  String get protectionStopped;

  /// No description provided for @working.
  ///
  /// In ru, this message translates to:
  /// **'Рабочий'**
  String get working;

  /// No description provided for @notWorking.
  ///
  /// In ru, this message translates to:
  /// **'Нерабочий'**
  String get notWorking;

  /// No description provided for @settings.
  ///
  /// In ru, this message translates to:
  /// **'Настройки'**
  String get settings;

  /// No description provided for @settingsSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Кастомизация внешнего вида и поведения приложения.'**
  String get settingsSubtitle;

  /// No description provided for @appearance.
  ///
  /// In ru, this message translates to:
  /// **'Внешний вид'**
  String get appearance;

  /// No description provided for @themePreset.
  ///
  /// In ru, this message translates to:
  /// **'Пресет темы'**
  String get themePreset;

  /// No description provided for @themeGroupDark.
  ///
  /// In ru, this message translates to:
  /// **'Тёмные'**
  String get themeGroupDark;

  /// No description provided for @themeGroupLight.
  ///
  /// In ru, this message translates to:
  /// **'Светлые'**
  String get themeGroupLight;

  /// No description provided for @backgroundMode.
  ///
  /// In ru, this message translates to:
  /// **'Режим фона'**
  String get backgroundMode;

  /// No description provided for @backgroundModeAuto.
  ///
  /// In ru, this message translates to:
  /// **'Авто (из темы)'**
  String get backgroundModeAuto;

  /// No description provided for @accentColor.
  ///
  /// In ru, this message translates to:
  /// **'Акцентный цвет'**
  String get accentColor;

  /// No description provided for @glowIntensity.
  ///
  /// In ru, this message translates to:
  /// **'Интенсивность свечения'**
  String get glowIntensity;

  /// No description provided for @blurStrength.
  ///
  /// In ru, this message translates to:
  /// **'Сила размытия (glass)'**
  String get blurStrength;

  /// No description provided for @backgroundSpeed.
  ///
  /// In ru, this message translates to:
  /// **'Скорость фоновых анимаций'**
  String get backgroundSpeed;

  /// No description provided for @particleDensity.
  ///
  /// In ru, this message translates to:
  /// **'Плотность частиц'**
  String get particleDensity;

  /// No description provided for @animationsEnabled.
  ///
  /// In ru, this message translates to:
  /// **'Анимации включены'**
  String get animationsEnabled;

  /// No description provided for @blackHoleEnabled.
  ///
  /// In ru, this message translates to:
  /// **'Чёрная дыра на фоне'**
  String get blackHoleEnabled;

  /// No description provided for @blackHole.
  ///
  /// In ru, this message translates to:
  /// **'Чёрная дыра'**
  String get blackHole;

  /// No description provided for @blackHoleRadius.
  ///
  /// In ru, this message translates to:
  /// **'Размер чёрной дыры'**
  String get blackHoleRadius;

  /// No description provided for @blackHoleDiskBrightness.
  ///
  /// In ru, this message translates to:
  /// **'Яркость аккреционного диска'**
  String get blackHoleDiskBrightness;

  /// No description provided for @blackHoleLensIntensity.
  ///
  /// In ru, this message translates to:
  /// **'Сила гравитационного линзирования'**
  String get blackHoleLensIntensity;

  /// No description provided for @behavior.
  ///
  /// In ru, this message translates to:
  /// **'Поведение'**
  String get behavior;

  /// No description provided for @minimizeToTray.
  ///
  /// In ru, this message translates to:
  /// **'Сворачивать в трей при закрытии'**
  String get minimizeToTray;

  /// No description provided for @autostart.
  ///
  /// In ru, this message translates to:
  /// **'Автозапуск при старте системы'**
  String get autostart;

  /// No description provided for @resetSettings.
  ///
  /// In ru, this message translates to:
  /// **'Сбросить настройки по умолчанию'**
  String get resetSettings;

  /// No description provided for @resetConfirmTitle.
  ///
  /// In ru, this message translates to:
  /// **'Сбросить настройки?'**
  String get resetConfirmTitle;

  /// No description provided for @resetConfirmMessage.
  ///
  /// In ru, this message translates to:
  /// **'Все параметры кастомизации вернутся к значениям по умолчанию.'**
  String get resetConfirmMessage;

  /// No description provided for @cancel.
  ///
  /// In ru, this message translates to:
  /// **'Отмена'**
  String get cancel;

  /// No description provided for @reset.
  ///
  /// In ru, this message translates to:
  /// **'Сбросить'**
  String get reset;

  /// No description provided for @yes.
  ///
  /// In ru, this message translates to:
  /// **'Да'**
  String get yes;

  /// No description provided for @no.
  ///
  /// In ru, this message translates to:
  /// **'Нет'**
  String get no;

  /// No description provided for @minimize.
  ///
  /// In ru, this message translates to:
  /// **'Свернуть'**
  String get minimize;

  /// No description provided for @maximize.
  ///
  /// In ru, this message translates to:
  /// **'Развернуть'**
  String get maximize;

  /// No description provided for @restore.
  ///
  /// In ru, this message translates to:
  /// **'Восстановить'**
  String get restore;

  /// No description provided for @close.
  ///
  /// In ru, this message translates to:
  /// **'Закрыть'**
  String get close;

  /// No description provided for @language.
  ///
  /// In ru, this message translates to:
  /// **'Язык'**
  String get language;

  /// No description provided for @russian.
  ///
  /// In ru, this message translates to:
  /// **'Русский'**
  String get russian;

  /// No description provided for @english.
  ///
  /// In ru, this message translates to:
  /// **'English'**
  String get english;

  /// No description provided for @liveLog.
  ///
  /// In ru, this message translates to:
  /// **'Live log'**
  String get liveLog;

  /// No description provided for @logTitle.
  ///
  /// In ru, this message translates to:
  /// **'ЖУРНАЛ'**
  String get logTitle;

  /// No description provided for @waitingToStart.
  ///
  /// In ru, this message translates to:
  /// **'Ожидание запуска...'**
  String get waitingToStart;

  /// No description provided for @statistics.
  ///
  /// In ru, this message translates to:
  /// **'Статистика'**
  String get statistics;

  /// No description provided for @status.
  ///
  /// In ru, this message translates to:
  /// **'Статус'**
  String get status;

  /// No description provided for @dpiBypass.
  ///
  /// In ru, this message translates to:
  /// **'DPI обход'**
  String get dpiBypass;

  /// No description provided for @telegramProxy.
  ///
  /// In ru, this message translates to:
  /// **'Telegram прокси'**
  String get telegramProxy;

  /// No description provided for @hosts.
  ///
  /// In ru, this message translates to:
  /// **'Hosts'**
  String get hosts;

  /// No description provided for @activeProcesses.
  ///
  /// In ru, this message translates to:
  /// **'Активных процессов'**
  String get activeProcesses;

  /// No description provided for @activeProcessesCount.
  ///
  /// In ru, this message translates to:
  /// **'{count} процессов'**
  String activeProcessesCount(int count);

  /// No description provided for @selectedConfigs.
  ///
  /// In ru, this message translates to:
  /// **'Выбрано конфигов'**
  String get selectedConfigs;

  /// No description provided for @version.
  ///
  /// In ru, this message translates to:
  /// **'Версия'**
  String get version;

  /// No description provided for @versionWithValue.
  ///
  /// In ru, this message translates to:
  /// **'Версия: {version}'**
  String versionWithValue(String version);

  /// No description provided for @uptime.
  ///
  /// In ru, this message translates to:
  /// **'Время работы'**
  String get uptime;

  /// No description provided for @noActiveProcesses.
  ///
  /// In ru, this message translates to:
  /// **'Нет активных процессов'**
  String get noActiveProcesses;

  /// No description provided for @selectedConfigsTitle.
  ///
  /// In ru, this message translates to:
  /// **'ВЫБРАННЫЕ КОНФИГИ'**
  String get selectedConfigsTitle;

  /// No description provided for @restart.
  ///
  /// In ru, this message translates to:
  /// **'Перезапустить'**
  String get restart;

  /// No description provided for @configFolder.
  ///
  /// In ru, this message translates to:
  /// **'Папка конфигов'**
  String get configFolder;

  /// No description provided for @singularityStatus.
  ///
  /// In ru, this message translates to:
  /// **'SINGULARITY STATUS'**
  String get singularityStatus;

  /// No description provided for @installed.
  ///
  /// In ru, this message translates to:
  /// **'Установлен'**
  String get installed;

  /// No description provided for @notInstalled.
  ///
  /// In ru, this message translates to:
  /// **'Не установлен'**
  String get notInstalled;

  /// No description provided for @logEmpty.
  ///
  /// In ru, this message translates to:
  /// **'Лог пуст'**
  String get logEmpty;

  /// No description provided for @menu.
  ///
  /// In ru, this message translates to:
  /// **'МЕНЮ'**
  String get menu;

  /// No description provided for @aiTitle.
  ///
  /// In ru, this message translates to:
  /// **'ИИ обход'**
  String get aiTitle;

  /// No description provided for @aiSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Разблокировка ChatGPT, Claude, Gemini через hosts-файл.'**
  String get aiSubtitle;

  /// No description provided for @provider.
  ///
  /// In ru, this message translates to:
  /// **'ПРОВАЙДЕР'**
  String get provider;

  /// No description provided for @install.
  ///
  /// In ru, this message translates to:
  /// **'Установить'**
  String get install;

  /// No description provided for @update.
  ///
  /// In ru, this message translates to:
  /// **'Обновить'**
  String get update;

  /// No description provided for @uninstall.
  ///
  /// In ru, this message translates to:
  /// **'Удалить'**
  String get uninstall;

  /// No description provided for @check.
  ///
  /// In ru, this message translates to:
  /// **'Проверить'**
  String get check;

  /// No description provided for @installedUpToDate.
  ///
  /// In ru, this message translates to:
  /// **'Установлено, актуально'**
  String get installedUpToDate;

  /// No description provided for @installedOutdated.
  ///
  /// In ru, this message translates to:
  /// **'Установлено, но устарело'**
  String get installedOutdated;

  /// No description provided for @aiNotInstalled.
  ///
  /// In ru, this message translates to:
  /// **'Не установлено'**
  String get aiNotInstalled;

  /// No description provided for @uninstallAiTitle.
  ///
  /// In ru, this message translates to:
  /// **'Удалить ИИ-обход?'**
  String get uninstallAiTitle;

  /// No description provided for @uninstallAiMessage.
  ///
  /// In ru, this message translates to:
  /// **'hosts-файл будет восстановлен к значению по умолчанию.'**
  String get uninstallAiMessage;

  /// No description provided for @telegramTitle.
  ///
  /// In ru, this message translates to:
  /// **'Telegram обход'**
  String get telegramTitle;

  /// No description provided for @telegramSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'MTProto WebSocket-мост для разблокировки Telegram Desktop.'**
  String get telegramSubtitle;

  /// No description provided for @tgProxyNotFoundTitle.
  ///
  /// In ru, this message translates to:
  /// **'TgWsProxy.exe не найден'**
  String get tgProxyNotFoundTitle;

  /// No description provided for @tgProxyNotFoundMessage.
  ///
  /// In ru, this message translates to:
  /// **'Положите TgWsProxy_windows.exe в папку bin/ и перезапустите приложение.'**
  String get tgProxyNotFoundMessage;

  /// No description provided for @proxySettings.
  ///
  /// In ru, this message translates to:
  /// **'НАСТРОЙКИ ПРОКСИ'**
  String get proxySettings;

  /// No description provided for @port.
  ///
  /// In ru, this message translates to:
  /// **'Порт'**
  String get port;

  /// No description provided for @fakeTlsDomainOptional.
  ///
  /// In ru, this message translates to:
  /// **'Fake TLS домен (опц.)'**
  String get fakeTlsDomainOptional;

  /// No description provided for @notSet.
  ///
  /// In ru, this message translates to:
  /// **'не задан'**
  String get notSet;

  /// No description provided for @connectionLink.
  ///
  /// In ru, this message translates to:
  /// **'ССЫЛКА ПОДКЛЮЧЕНИЯ'**
  String get connectionLink;

  /// No description provided for @openInTelegram.
  ///
  /// In ru, this message translates to:
  /// **'Открыть в Telegram'**
  String get openInTelegram;

  /// No description provided for @copy.
  ///
  /// In ru, this message translates to:
  /// **'Копировать'**
  String get copy;

  /// No description provided for @instructions.
  ///
  /// In ru, this message translates to:
  /// **'ИНСТРУКЦИЯ'**
  String get instructions;

  /// No description provided for @telegramInstructions.
  ///
  /// In ru, this message translates to:
  /// **'1. Нажмите START для запуска прокси\n2. Нажмите «Открыть в Telegram» или скопируйте ссылку\n3. В Telegram: Настройки → Продвинутые → Тип подключения → Прокси'**
  String get telegramInstructions;

  /// No description provided for @listsTitle.
  ///
  /// In ru, this message translates to:
  /// **'Редактор списков'**
  String get listsTitle;

  /// No description provided for @listsSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Редактирование доменов и IP для обхода блокировок.'**
  String get listsSubtitle;

  /// No description provided for @importListDialogTitle.
  ///
  /// In ru, this message translates to:
  /// **'Импорт списка'**
  String get importListDialogTitle;

  /// No description provided for @exportListDialogTitle.
  ///
  /// In ru, this message translates to:
  /// **'Экспорт списка'**
  String get exportListDialogTitle;

  /// No description provided for @selectListHint.
  ///
  /// In ru, this message translates to:
  /// **'Выберите список'**
  String get selectListHint;

  /// No description provided for @entriesCount.
  ///
  /// In ru, this message translates to:
  /// **'{count} записей'**
  String entriesCount(int count);

  /// No description provided for @domain.
  ///
  /// In ru, this message translates to:
  /// **'Домен'**
  String get domain;

  /// No description provided for @ip.
  ///
  /// In ru, this message translates to:
  /// **'IP'**
  String get ip;

  /// No description provided for @comment.
  ///
  /// In ru, this message translates to:
  /// **'Комментарий'**
  String get comment;

  /// No description provided for @save.
  ///
  /// In ru, this message translates to:
  /// **'Сохранить'**
  String get save;

  /// No description provided for @clearListTitle.
  ///
  /// In ru, this message translates to:
  /// **'Очистить список?'**
  String get clearListTitle;

  /// No description provided for @clearListMessage.
  ///
  /// In ru, this message translates to:
  /// **'Все записи будут удалены. Это действие нельзя отменить.'**
  String get clearListMessage;

  /// No description provided for @clear.
  ///
  /// In ru, this message translates to:
  /// **'Очистить'**
  String get clear;

  /// No description provided for @removeDuplicates.
  ///
  /// In ru, this message translates to:
  /// **'Удалить дубликаты'**
  String get removeDuplicates;

  /// No description provided for @listEntryHint.
  ///
  /// In ru, this message translates to:
  /// **'Домен или IP'**
  String get listEntryHint;

  /// No description provided for @commentHint.
  ///
  /// In ru, this message translates to:
  /// **'Комментарий'**
  String get commentHint;

  /// No description provided for @profilesTitle.
  ///
  /// In ru, this message translates to:
  /// **'Профили'**
  String get profilesTitle;

  /// No description provided for @profilesSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Сохранённые наборы настроек.'**
  String get profilesSubtitle;

  /// No description provided for @profile.
  ///
  /// In ru, this message translates to:
  /// **'Профиль'**
  String get profile;

  /// No description provided for @newProfile.
  ///
  /// In ru, this message translates to:
  /// **'Новый профиль'**
  String get newProfile;

  /// No description provided for @profileName.
  ///
  /// In ru, this message translates to:
  /// **'Название профиля'**
  String get profileName;

  /// No description provided for @create.
  ///
  /// In ru, this message translates to:
  /// **'Создать'**
  String get create;

  /// No description provided for @delete.
  ///
  /// In ru, this message translates to:
  /// **'Удалить'**
  String get delete;

  /// No description provided for @deleteProfileTitle.
  ///
  /// In ru, this message translates to:
  /// **'Удалить профиль?'**
  String get deleteProfileTitle;

  /// No description provided for @deleteProfileMessage.
  ///
  /// In ru, this message translates to:
  /// **'Профиль «{name}» будет удалён без возможности восстановления.'**
  String deleteProfileMessage(String name);

  /// No description provided for @activate.
  ///
  /// In ru, this message translates to:
  /// **'Активировать'**
  String get activate;

  /// No description provided for @profileCategoriesWithProvider.
  ///
  /// In ru, this message translates to:
  /// **'{count} категорий · {provider}'**
  String profileCategoriesWithProvider(int count, String provider);

  /// No description provided for @noProfiles.
  ///
  /// In ru, this message translates to:
  /// **'Нет сохранённых профилей'**
  String get noProfiles;

  /// No description provided for @copyLink.
  ///
  /// In ru, this message translates to:
  /// **'Скопировать ссылку'**
  String get copyLink;

  /// No description provided for @proxyPort.
  ///
  /// In ru, this message translates to:
  /// **'Порт прокси'**
  String get proxyPort;

  /// No description provided for @portWithValue.
  ///
  /// In ru, this message translates to:
  /// **'Порт {port}'**
  String portWithValue(int port);

  /// No description provided for @fakeTlsDomain.
  ///
  /// In ru, this message translates to:
  /// **'Fake TLS домен'**
  String get fakeTlsDomain;

  /// No description provided for @updateAvailable.
  ///
  /// In ru, this message translates to:
  /// **'Доступно обновление'**
  String get updateAvailable;

  /// No description provided for @versionDate.
  ///
  /// In ru, this message translates to:
  /// **'Опубликовано: {date}'**
  String versionDate(String date);

  /// No description provided for @noDescription.
  ///
  /// In ru, this message translates to:
  /// **'Нет описания'**
  String get noDescription;

  /// No description provided for @downloadingUpdate.
  ///
  /// In ru, this message translates to:
  /// **'Загрузка обновления...'**
  String get downloadingUpdate;

  /// No description provided for @download.
  ///
  /// In ru, this message translates to:
  /// **'Скачать'**
  String get download;

  /// No description provided for @later.
  ///
  /// In ru, this message translates to:
  /// **'Позже'**
  String get later;

  /// No description provided for @folder.
  ///
  /// In ru, this message translates to:
  /// **'Папка'**
  String get folder;

  /// No description provided for @runInstaller.
  ///
  /// In ru, this message translates to:
  /// **'Запустить установщик'**
  String get runInstaller;

  /// No description provided for @fileHash.
  ///
  /// In ru, this message translates to:
  /// **'SHA256'**
  String get fileHash;

  /// No description provided for @hashHint.
  ///
  /// In ru, this message translates to:
  /// **'Файл загружен. Сверьте SHA256 с релизом на GitHub перед запуском.'**
  String get hashHint;

  /// No description provided for @errorWithMessage.
  ///
  /// In ru, this message translates to:
  /// **'Ошибка: {error}'**
  String errorWithMessage(String error);

  /// No description provided for @onboardingTitle.
  ///
  /// In ru, this message translates to:
  /// **'Добро пожаловать в Obsession'**
  String get onboardingTitle;

  /// No description provided for @onboardingSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Обход DPI и блокировок с космическим интерфейсом. Нажмите ниже, чтобы начать.'**
  String get onboardingSubtitle;

  /// No description provided for @onboardingContinue.
  ///
  /// In ru, this message translates to:
  /// **'Продолжить'**
  String get onboardingContinue;

  /// No description provided for @onboardingIntro.
  ///
  /// In ru, this message translates to:
  /// **'Космический лаунчер для обхода блокировок. DPI, ИИ и Telegram — всё в одном окне.'**
  String get onboardingIntro;

  /// No description provided for @onboardingDpi.
  ///
  /// In ru, this message translates to:
  /// **'Автоматический подбор конфигураций WinDivert для Discord, YouTube, игр и других сервисов.'**
  String get onboardingDpi;

  /// No description provided for @onboardingAi.
  ///
  /// In ru, this message translates to:
  /// **'Разблокировка ChatGPT, Claude и Gemini через обновление hosts-файла.'**
  String get onboardingAi;

  /// No description provided for @onboardingTelegram.
  ///
  /// In ru, this message translates to:
  /// **'Локальный MTProto WebSocket-прокси для разблокировки Telegram Desktop.'**
  String get onboardingTelegram;

  /// No description provided for @onboardingProfiles.
  ///
  /// In ru, this message translates to:
  /// **'Сохраняйте настройки и переключайтесь между ними одним кликом.'**
  String get onboardingProfiles;

  /// No description provided for @onboardingStyleTitle.
  ///
  /// In ru, this message translates to:
  /// **'Настрой свой стиль'**
  String get onboardingStyleTitle;

  /// No description provided for @onboardingStyle.
  ///
  /// In ru, this message translates to:
  /// **'Акцентный цвет, свечение, чёрная дыра и анимации — под твоё настроение.'**
  String get onboardingStyle;

  /// No description provided for @skip.
  ///
  /// In ru, this message translates to:
  /// **'Пропустить'**
  String get skip;

  /// No description provided for @next.
  ///
  /// In ru, this message translates to:
  /// **'Далее'**
  String get next;

  /// No description provided for @getStarted.
  ///
  /// In ru, this message translates to:
  /// **'Начать'**
  String get getStarted;

  /// No description provided for @open.
  ///
  /// In ru, this message translates to:
  /// **'Открыть'**
  String get open;

  /// No description provided for @availability.
  ///
  /// In ru, this message translates to:
  /// **'Доступность'**
  String get availability;

  /// No description provided for @latencyMs.
  ///
  /// In ru, this message translates to:
  /// **'{ms} мс'**
  String latencyMs(int ms);

  /// No description provided for @unreachable.
  ///
  /// In ru, this message translates to:
  /// **'недоступен'**
  String get unreachable;

  /// No description provided for @checking.
  ///
  /// In ru, this message translates to:
  /// **'проверка...'**
  String get checking;

  /// No description provided for @emergencyStop.
  ///
  /// In ru, this message translates to:
  /// **'Экстренный стоп'**
  String get emergencyStop;

  /// No description provided for @emergencyStopTitle.
  ///
  /// In ru, this message translates to:
  /// **'Остановить всё?'**
  String get emergencyStopTitle;

  /// No description provided for @emergencyStopMessage.
  ///
  /// In ru, this message translates to:
  /// **'Будут остановлены DPI, Telegram-прокси и удалены hosts-правила. Продолжить?'**
  String get emergencyStopMessage;

  /// No description provided for @saveLogToFile.
  ///
  /// In ru, this message translates to:
  /// **'Сохранить лог в файл'**
  String get saveLogToFile;

  /// No description provided for @logSavedTo.
  ///
  /// In ru, this message translates to:
  /// **'Лог сохранён: {path}'**
  String logSavedTo(String path);

  /// No description provided for @logSaveFailed.
  ///
  /// In ru, this message translates to:
  /// **'Не удалось сохранить лог'**
  String get logSaveFailed;

  /// No description provided for @autoDetected.
  ///
  /// In ru, this message translates to:
  /// **'Авто-обнаруженные'**
  String get autoDetected;

  /// No description provided for @autoDetectedSubtitle.
  ///
  /// In ru, this message translates to:
  /// **'Домены, которые Zapret сам определил как заблокированные'**
  String get autoDetectedSubtitle;

  /// No description provided for @noAutoDetected.
  ///
  /// In ru, this message translates to:
  /// **'Пока ничего не обнаружено. Список формируется автоматически при использовании обхода.'**
  String get noAutoDetected;

  /// No description provided for @clearAutoDetected.
  ///
  /// In ru, this message translates to:
  /// **'Очистить списки'**
  String get clearAutoDetected;

  /// No description provided for @autoDetectedCleared.
  ///
  /// In ru, this message translates to:
  /// **'Списки авто-обнаружения очищены'**
  String get autoDetectedCleared;

  /// No description provided for @domainsCount.
  ///
  /// In ru, this message translates to:
  /// **'{count} доменов'**
  String domainsCount(int count);

  /// No description provided for @orphanDetectedTitle.
  ///
  /// In ru, this message translates to:
  /// **'Обнаружены зависшие процессы'**
  String get orphanDetectedTitle;

  /// No description provided for @orphanDetectedMessage.
  ///
  /// In ru, this message translates to:
  /// **'Найдено {count} процесс(ов) winws.exe от предыдущего запуска (возможно, после краша). Они могут мешать запуску обхода. Остановить их?\n\nВнимание: будут остановлены ВСЕ процессы winws.exe в системе, включая запущенные другими DPI-инструментами.'**
  String orphanDetectedMessage(int count);

  /// No description provided for @orphanDetectedClear.
  ///
  /// In ru, this message translates to:
  /// **'Остановить'**
  String get orphanDetectedClear;

  /// No description provided for @orphanDetectedIgnore.
  ///
  /// In ru, this message translates to:
  /// **'Игнорировать'**
  String get orphanDetectedIgnore;

  /// No description provided for @orphanDetectedNone.
  ///
  /// In ru, this message translates to:
  /// **'Зависших процессов не обнаружено.'**
  String get orphanDetectedNone;

  /// No description provided for @orphanDetectedCleared.
  ///
  /// In ru, this message translates to:
  /// **'Зависшие процессы остановлены.'**
  String get orphanDetectedCleared;

  /// No description provided for @orphanDetectedCleanFailed.
  ///
  /// In ru, this message translates to:
  /// **'Не удалось остановить зависшие процессы.'**
  String get orphanDetectedCleanFailed;
}

class _AppLocalizationsDelegate
    extends LocalizationsDelegate<AppLocalizations> {
  const _AppLocalizationsDelegate();

  @override
  Future<AppLocalizations> load(Locale locale) {
    return SynchronousFuture<AppLocalizations>(lookupAppLocalizations(locale));
  }

  @override
  bool isSupported(Locale locale) =>
      <String>['en', 'ru'].contains(locale.languageCode);

  @override
  bool shouldReload(_AppLocalizationsDelegate old) => false;
}

AppLocalizations lookupAppLocalizations(Locale locale) {
  // Lookup logic when only language code is specified.
  switch (locale.languageCode) {
    case 'en':
      return AppLocalizationsEn();
    case 'ru':
      return AppLocalizationsRu();
  }

  throw FlutterError(
    'AppLocalizations.delegate failed to load unsupported locale "$locale". This is likely '
    'an issue with the localizations generation tool. Please file an issue '
    'on GitHub with a reproducible sample app and the gen-l10n configuration '
    'that was used.',
  );
}

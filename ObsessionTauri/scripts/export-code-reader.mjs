#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Publish a reproducible reading snapshot, not working-tree files or build output.
const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const outputDirectory = path.join(repositoryRoot, "ObsessionTauri/docs/code-reader");
const args = process.argv.slice(2);
const checkOnly = args.includes("--check");
const ref = args.find((arg) => !arg.startsWith("--")) ?? "v1.2.0";
assert(args.every((arg) => arg === ref || arg === "--check"), "Usage: node scripts/export-code-reader.mjs [ref] [--check]");
function git(...arguments_) {
  const result = spawnSync("git", arguments_, { cwd: repositoryRoot, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(result.stderr.trim() || "Git command failed");
  return result.stdout;
}
const commit = git("rev-parse", "--verify", `${ref}^{commit}`).trim();
assert(/^[0-9a-f]{40}$/.test(commit));
const sourceDate = git("show", "-s", "--format=%cs", commit).trim();
const version = JSON.parse(git("show", `${commit}:ObsessionTauri/package.json`)).version;
const repository = "AcediaWhy/Obsession";
const blob = (file) => `https://github.com/${repository}/blob/${commit}/${file}`;
const tree = (directory) => `https://github.com/${repository}/tree/${commit}/${directory}`;
const raw = (file) => `https://raw.githubusercontent.com/${repository}/${commit}/${file}`;
const digest = (text) => createHash("sha256").update(text, "utf8").digest("hex");
const source = (file) => git("show", `${commit}:${file}`);
const module_ = (file, purpose) => ({ file: `ObsessionTauri/${file}`, purpose });
const packs = [
  {
    name: "installer", title: "Установщик: установка, обновление и откат",
    intro: "Здесь собран путь от окна Setup до привилегированного Windows-worker. Основной интерес — замена приложения и службы с журналом состояния, проверкой файлов и восстановлением после прерывания.",
    flow: "flowchart TD\n  UI[Окно Setup] --> HANDOFF[machine_handoff: запрос UAC]\n  HANDOFF --> WORKER[machine_worker: установка и удаление]\n  WORKER --> CHECK[Проверка payload и каталогов]\n  CHECK --> STAGE[Подготовка нового каталога]\n  STAGE --> SWAP[Остановка службы и замена установки]\n  SWAP --> VERIFY[Запуск службы и проверка]\n  SWAP --> RECOVER[Журнал и восстановление]\n  UI --> USER[upgrade: пользовательские настройки]",
    reading: [
      "Начни с installer/src-tauri/src/lib.rs: там видна развилка release/debug и точки вызова worker.",
      "Затем прочитай machine_handoff.rs и machine_worker.rs: повышение прав, владение файлами, фазы установки и обновления.",
      "upgrade.rs также содержит прежний transactional flow для debug-симуляции. В release основной machine install/update выполняется через native worker; пользовательские настройки и миграции по-прежнему используют функции upgrade.rs.",
      "payload_compression.rs и build-setup.mjs связывают упаковку при сборке с проверкой и распаковкой при установке.",
    ],
    attention: [
      "Какая фаза записана в журнале до изменения файлов и как выбирается действие при следующем запуске.",
      "Что происходит при отмене UAC, занятых файлах, ошибке запуска службы или неполной записи журнала.",
      "Как проверяются права каталогов, reparse points, идентичность Setup и принадлежность удаляемых файлов.",
    ],
    modules: [
      module_("installer/src-tauri/src/lib.rs", "Команды Setup, release/debug-ветки и пользовательская часть установки."),
      module_("installer/src-tauri/src/machine_handoff.rs", "Передача операции native worker через UAC и ожидание результата."),
      module_("installer/src-tauri/src/machine_worker.rs", "Machine install/update/uninstall, payload, журнал, каталоги и служба."),
      module_("installer/src-tauri/src/installed_app.rs", "Поиск и проверка работающего установленного приложения."),
      module_("installer/src-tauri/src/payload_compression.rs", "Распаковка ограниченного по размеру payload и проверка хеша."),
      module_("installer/src-tauri/src/upgrade.rs", "Планирование, пользовательские настройки, миграции и debug-транзакции."),
      module_("scripts/build-setup.mjs", "Сборка приложения, ресурсов, Setup и подписанного обновления."),
    ],
    related: [
      module_("installer/src-tauri/src/user_cleanup.rs", "Очистка пользовательских данных."),
      module_("installer/src-tauri/src/uninstall.rs", "Пользовательская часть удаления."),
      module_("installer/src-tauri/src/webview_bootstrap.rs", "Проверка и установка WebView2."),
      module_("runtime-service/src/protected_layout.rs", "Защищённые каталоги и проверка владения."),
      module_("installer/PAYLOAD_COMPRESSION.md", "Описание формата пакета."),
      module_("installer/UNINSTALL.md", "Границы удаления."),
      module_("installer/src-tauri/Cargo.toml", "Зависимости установщика."),
    ],
  },
  {
    name: "runtime", title: "Runtime: служба, процессы и восстановление",
    intro: "Этот пакет показывает, как запрос приложения проходит через защищённый named pipe к Windows-службе и превращается в операции с процессами, TCP-настройками или hosts. Здесь важны владение ресурсами, согласованность состояния и длительность операций.",
    flow: "flowchart TD\n  APP[Приложение] --> PIPE[named_pipe: клиент и IPC]\n  SCM[scm: жизненный цикл службы] --> PIPE\n  PIPE --> CORE[lib: dispatch]\n  CORE --> BACKEND[protected_backend]\n  BACKEND --> EXEC[dpi_executor: группа процессов]\n  EXEC --> TS[tcp_timestamps: глобальная настройка]\n  BACKEND --> HOSTS[hosts_controller: ИИ-маршруты]\n  BACKEND --> RECOVERY[legacy_recovery_transaction]",
    reading: [
      "Начни с runtime-service/src/lib.rs: запросы, ответы, ошибки и интерфейс backend. Полный IPC-контракт доступен по ссылке на runtime-protocol ниже.",
      "scm.rs и named_pipe.rs показывают запуск службы, проверку клиента и порядок обработки запросов.",
      "protected_backend.rs связывает компоненты; dpi_executor.rs управляет процессами и Windows Job Objects.",
      "tcp_timestamps.rs, hosts_controller.rs и legacy_recovery_transaction.rs позволяют отдельно изучить восстановление глобальной настройки, запись hosts и выполнение recovery-операций.",
    ],
    attention: [
      "Какие проверки относятся к целостности и владению, а какие — к доступности сети.",
      "Где выполняются блокирующие операции, как ограничено ожидание и могут ли в это время обслуживаться другие запросы.",
      "Что остаётся после ошибки: процесс, каталог, журнал, hosts и состояние клиента.",
    ],
    modules: [
      module_("runtime-service/src/lib.rs", "Dispatch, типы ошибок и контракт backend."),
      module_("runtime-service/src/scm.rs", "Запуск, остановка и транспорт Windows-службы."),
      module_("runtime-service/src/named_pipe.rs", "Аутентификация клиента, чтение IPC-фреймов и обработка запросов."),
      module_("runtime-service/src/protected_backend.rs", "Связь службы с процессами, hosts и recovery."),
      module_("runtime-service/src/dpi_executor.rs", "Запуск, идентичность, Job Objects и завершение процессов."),
      module_("runtime-service/src/tcp_timestamps.rs", "Журнал и временное изменение TCP timestamps для Legacy."),
      module_("runtime-service/src/hosts_controller.rs", "Получение ИИ-маршрутов, проверки, запись hosts и откат."),
      module_("runtime-service/src/legacy_recovery_transaction.rs", "Переходы recovery-транзакции и передача preflight."),
    ],
    related: [
      module_("runtime-protocol/src/lib.rs", "Полный IPC-контракт и его валидация."),
      module_("runtime-client/src/lib.rs", "Клиент службы."),
      module_("runtime-service/src/dpi_materializer.rs", "Подготовка каталога и файлов запуска."),
      module_("runtime-service/src/legacy_recovery_effects.rs", "Применение конкретных recovery-действий."),
      module_("src-tauri/src/legacy_reliability/executor.rs", "Проверки актуальности, подтверждение кандидатов и откат."),
      module_("runtime-service/src/legacy_confirmation.rs", "Подтверждение восстановления Legacy."),
      module_("runtime-service/src/dns_recovery.rs", "Управление старым журналом системного DNS."),
      module_("runtime-service/Cargo.toml", "Зависимости службы."),
      module_("docs/SECURE_RUNTIME_ARCHITECTURE.md", "Описание архитектуры."),
    ],
  },
  {
    name: "zapret2", title: "Zapret2: профили, методы и подбор",
    intro: "Пакет соединяет встроенные профили, Lua-методы движка, подготовку команд и алгоритм подбора. Он позволяет проследить путь от выбора сервиса до измерений, сравнения кандидатов и применения результата.",
    flow: "flowchart TD\n  MANIFEST[manifest.json: профили и рецепты] --> PACK[zapret2_pack: проверка и компиляция]\n  RECIPE[zapret2_recipe: типы] --> PACK\n  PICKER[zapret2_picker: подбор] --> PACK\n  PACK --> ENGINE[winws2 и Lua-методы]\n  PICKER --> MEASURE[zapret2_measure: раунды]\n  MEASURE --> TOURNAMENT[zapret2_tournament: сравнение]\n  TOURNAMENT --> PICKER\n  PICKER --> APPLY[Явное применение результата]",
    reading: [
      "manifest.json содержит встроенные стратегии, аргументы и рецепты. Lua-файлы ниже содержат функции, которые использует движок.",
      "zapret2_recipe.rs задаёт типы рецептов; zapret2_pack.rs проверяет пакет и формирует команды для службы.",
      "В zapret2_picker.rs найди catalog, tournament_candidates, pick и apply_result: каталог, кандидаты, подбор и применение.",
      "zapret2_measure.rs хранит результаты раундов и выполняет сетевые измерения; zapret2_tournament.rs сравнивает кандидатов и требует подтверждения результата.",
    ],
    attention: [
      "Чем отличаются доступный endpoint, обработанный движком сокет и работа видео или голосового сценария.",
      "Какие раунды допускаются к сравнению, как учитываются смена сети, отмена и неполные измерения.",
      "Как временная конфигурация возвращается после проверки и чем новая проверка отличается от сохранённого результата.",
    ],
    modules: [
      module_("src-tauri/resources/strategy-packs/builtin/manifest.json", "Все определения встроенного пака: стратегии, аргументы и рецепты."),
      module_("runtime-protocol/src/zapret2_recipe.rs", "Типизированные рецепты и источники кандидатов."),
      module_("runtime-service/src/zapret2_pack.rs", "Проверка и компиляция профилей для службы."),
      module_("src-tauri/src/protected_runtime/zapret2_picker.rs", "Каталог, подготовка кандидатов, подбор и применение."),
      module_("src-tauri/src/protected_runtime/zapret2_tournament.rs", "Разнообразие кандидатов, подтверждения и выбор результата."),
      module_("src-tauri/src/protected_runtime/zapret2_measure.rs", "Измерения, сравнимость и доказательства по функциям."),
      module_("src-tauri/resources/strategy-packs/builtin/lua/zapret-antidpi.lua", "Lua-функции обхода, поставляемые с движком."),
      module_("src-tauri/resources/strategy-packs/builtin/lua/zapret-lib.lua", "Общие Lua-функции, используемые методами обхода."),
    ],
    related: [
      module_("src-tauri/src/protected_runtime/zapret2_routing.rs", "Подтверждение обработки точных соединений."),
      module_("src-tauri/src/protected_runtime/zapret2_tls.rs", "TLS-метаданные и проверки."),
      module_("src-tauri/src/protected_runtime/zapret2_quic.rs", "Проверки QUIC."),
      module_("src-tauri/src/protected_runtime/zapret2_composition.rs", "Составные конфигурации Discord."),
      module_("src-tauri/src/protected_runtime/youtube_playback.rs", "Отдельное испытание воспроизведения видео."),
      module_("src-tauri/src/protected_runtime/zapret2_history.rs", "Сохранение результатов."),
      module_("src/store/zapret2StrategyStore.ts", "Состояние подбора в интерфейсе."),
      module_("src/design/components/Zapret2StrategyPanel.tsx", "Интерфейс выбора и применения профилей."),
      module_("third-party/zapret2/README.md", "Происхождение движка, патчи и условия стороннего кода."),
      module_("src-tauri/Cargo.toml", "Зависимости приложения и измерений."),
    ],
  },
];

const outputs = new Map();
const metadata = { schema_version: 1, repository, source_ref: ref, source_commit: commit, source_date: sourceDate, app_version: version, documents: [] };
for (const pack of packs) {
  for (const entry of pack.related) git("cat-file", "-e", `${commit}:${entry.file}`);
  const records = pack.modules.map((entry, index) => {
    const text = source(entry.file);
    const runs = text.match(/`+/g) ?? [];
    const fence = "`".repeat(Math.max(3, ...runs.map((run) => run.length + 1)));
    return { ...entry, text, fence, id: `source-${String(index + 1).padStart(2, "0")}`, language: entry.file.endsWith(".rs") ? "rust" : entry.file.endsWith(".json") ? "json" : entry.file.endsWith(".lua") ? "lua" : "javascript", bytes: Buffer.byteLength(text), lines: text.split("\n").length - Number(text.endsWith("\n")), sha256: digest(text), ends_with_newline: text.endsWith("\n") };
  });
  let document = `# ${pack.title}\n\n[Все пакеты](README.md) · [Снимок исходников](https://github.com/${repository}/tree/${commit})\n\n**Obsession ${version} · исходники от ${sourceDate} · коммит \`${commit}\`.**\n\n${pack.intro}\n\nКлючевые файлы приведены целиком, включая комментарии и тесты. Код взят из указанного Git-коммита; пояснения и схемы добавлены для чтения. Все ссылки на исходники закреплены на тот же коммит.\n\n## Схема\n\n\`\`\`mermaid\n${pack.flow}\n\`\`\`\n\n## Порядок чтения\n\n${pack.reading.map((item, i) => `${i + 1}. ${item}`).join("\n")}\n\n## Карта файлов\n\n| Файл | Назначение | Строк |\n|---|---|---:|\n${records.map((record) => `| [${record.file}](#${record.id}) | ${record.purpose} | ${record.lines} |`).join("\n")}\n\n## На что обратить внимание\n\n${pack.attention.map((item) => `- ${item}`).join("\n")}\n\n## Исходники\n\nБлоки можно раскрыть по одному. В Raw-версии документа содержимое всех блоков доступно сразу.\n`;
  for (const record of records) {
    document += `\n<a id="${record.id}"></a>\n\n### ${record.file}\n\n${record.purpose}\n\n[Открыть файл на GitHub](${blob(record.file)}) · [Исходный текст](${raw(record.file)})\n\n${record.lines} строк · ${record.bytes.toLocaleString("en-US")} байт · SHA-256: \`${record.sha256}\`\n\n<details>\n<summary>Показать полный код ${path.posix.basename(record.file)}</summary>\n\n${record.fence}${record.language}\n${record.text}${record.ends_with_newline ? "" : "\n"}${record.fence}\n\n</details>\n`;
  }
  document += `\n## Связанные модули\n\n| Файл | Зачем открывать |\n|---|---|\n${pack.related.map((entry) => `| [${entry.file}](${blob(entry.file)}) | ${entry.purpose} |`).join("\n")}\n\n## Лицензии\n\nСобственный код проекта распространяется по [MPL-2.0](${blob("LICENSE")}). Для сторонних компонентов действуют их лицензии и указания авторов: [источники Zapret2](${blob("ObsessionTauri/third-party/zapret2/README.md")}), [лицензии поставки](${tree("ObsessionTauri/src-tauri/resources/licenses")}).\n`;
  const bytes = Buffer.byteLength(document);
  assert(bytes < 480_000, `${pack.name}.md is too large for a convenient GitHub page`);
  for (const record of records) {
    const startMarker = `${record.fence}${record.language}\n`;
    const start = document.indexOf(startMarker, document.indexOf(`<a id="${record.id}">`)) + startMarker.length;
    assert(start >= startMarker.length);
    const embedded = document.slice(start, start + record.text.length);
    assert.equal(digest(embedded), record.sha256, `Incomplete embedded source: ${record.file}`);
    assert(document.slice(start + record.text.length).startsWith(`${record.ends_with_newline ? "" : "\n"}${record.fence}\n`));
  }
  const record = { name: pack.name, file: `${pack.name}.md`, title: pack.title, bytes, lines: records.reduce((sum, entry) => sum + entry.lines, 0), sha256: digest(document), sources: records.map(({ text, ...entry }) => entry) };
  metadata.documents.push(record);
  outputs.set(record.file, document);
}

const docsRoot = "ObsessionTauri/docs/code-reader";
const index = `# Код Obsession для чтения и разбора\n\nТри самостоятельных Markdown-пакета: карта модуля, схема, ключевые файлы целиком и ссылки на соседние исходники.\n\n**Снимок Obsession ${version}: [${commit.slice(0, 7)}](https://github.com/${repository}/commit/${commit}), ${sourceDate}.** Код и ссылки на исходники закреплены на этом коммите. Программная часть проекта при создании пакетов не меняется.\n\n## Выбери пакет\n\n| Пакет | Что внутри | Файлов целиком | Объём | Текст для чата |\n|---|---|---:|---:|---|\n${metadata.documents.map((entry) => `| [${entry.title}](${entry.file}) | ${packs.find((pack) => pack.name === entry.name).intro} | ${entry.sources.length} | ${(entry.bytes / 1024).toFixed(0)} КиБ | [Raw](https://raw.githubusercontent.com/${repository}/refs/heads/main/${docsRoot}/${entry.file}) |`).join("\n")}\n\n## Как поделиться\n\n**Другу:** отправь ссылку на страницу нужного пакета. Код свёрнут по файлам; открой интересующий блок. Схема и карта в начале объясняют связи между модулями.\n\n**Нейронке:** передай Raw-ссылку из таблицы или сохрани её содержимое как \`.md\` и прикрепи в чат. Пакеты объёмные — лучше отправлять по одному и указывать конкретный вопрос. Если модель не умеет открывать ссылки, используй файл.\n\nСсылки Raw в таблице ведут на текущие документы в \`main\`; версия вложенных исходников указана в каждом документе. Для неизменяемой ссылки на сам документ замени \`main\` или \`refs/heads/main\` на хеш коммита, добавившего пакеты.\n\n## Полнота и происхождение\n\n- Во встроенных файлах сохранены комментарии, тесты и исходные строки. Между ними добавлены только пояснения и Markdown-разметка.\n- В каждом пакете есть прямые ссылки на исходные файлы и дополнительные модули того же коммита.\n- Набор файлов, размеры и SHA-256 записаны в [snapshot.json](snapshot.json).\n- Зависимости, сборка и проверки описаны в [руководстве разработчика](${blob("ObsessionTauri/README.md")}).\n\n## Обновление пакетов\n\nИз корня репозитория:\n\n\`\`\`powershell\nnode ObsessionTauri/scripts/export-code-reader.mjs ${ref}\nnode ObsessionTauri/scripts/export-code-reader.mjs ${ref} --check\n\`\`\`\n\nПервая команда собирает документы из Git-коммита, вторая проверяет их соответствие источнику без записи файлов. Для нового снимка укажи другой существующий тег или коммит.\n`;
outputs.set("README.md", index);
outputs.set("snapshot.json", `${JSON.stringify(metadata, null, 2)}\n`);
if (!checkOnly) fs.mkdirSync(outputDirectory, { recursive: true });
for (const [file, content] of outputs) {
  const destination = path.join(outputDirectory, file);
  if (checkOnly) assert.equal(fs.readFileSync(destination, "utf8"), content, `Snapshot is stale: ${file}`);
  else fs.writeFileSync(destination, content, "utf8");
}
for (const entry of metadata.documents) console.log(`${entry.file}: ${entry.sources.length} complete files, ${entry.lines} source lines, ${entry.bytes} bytes`);
console.log(`${checkOnly ? "Verified" : "Created"} source snapshot ${commit}`);

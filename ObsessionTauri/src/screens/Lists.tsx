import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useShallow } from "zustand/react/shallow";

import { useListsStore, isDirty } from "../store/listsStore";
import { type ListInfo } from "../lib/tauri";
import { GlassPanel } from "../design/components/GlassPanel";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { Button, SectionLabel, TextField } from "../design/components/atoms";
import { Icon } from "../design/components/icons";

// Человеко-читаемые названия штатных списков.
const LIST_LABELS: Record<string, string> = {
  "black-list": "Чёрный список",
  "white-list": "Белый список",
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  "ipset-gaming": "IP-набор (Gaming)",
  "ipset-global": "IP-набор (Global)",
};

function label(name: string): string {
  return LIST_LABELS[name] ?? name;
}

export function ListsScreen() {
  const s = useListsStore(useShallow((state) => ({
    lists: state.lists,
    loaded: state.loaded,
    selected: state.selected,
    draft: state.draft,
    loading: state.loading,
    saving: state.saving,
    error: state.error,
    bootstrap: state.bootstrap,
    select: state.select,
    setDraft: state.setDraft,
    save: state.save,
    revert: state.revert,
    create: state.create,
    remove: state.remove,
  })));
  const dirty = useListsStore(isDirty);
  const current = s.lists.find((l) => l.name === s.selected);

  useEffect(() => {
    if (!s.loaded) s.bootstrap();
  }, []);

  // Число значимых строк в черновике (без пустых и комментариев).
  const draftEntries = s.draft
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("#")).length;

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Заголовок. */}
      <StaggerItem standalone>
        <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Списки</h1>
        <p className="text-sm text-ink-muted">
          Домены и IP, попадающие под DPI-обход · по строке на запись, `#` — комментарий
        </p>
      </StaggerItem>

      <div className="grid flex-1 grid-cols-[300px_1fr] gap-4 overflow-hidden">
        {/* Левая колонка: файлы + создание. */}
        <GlassPanel className="flex flex-col gap-3 overflow-hidden">
          <NewListForm
            onCreate={(n) => s.create(n)}
            disabled={dirty}
            disabledHint={dirty ? "Сохраните текущие правки" : ""}
          />
          <SectionLabel>Списки</SectionLabel>
          <div className="-mr-1 flex-1 overflow-y-auto pr-1">
            {!s.loaded ? (
              <div className="flex h-full items-center justify-center text-sm text-ink-muted">
                Загрузка…
              </div>
            ) : s.lists.length === 0 ? (
              <div className="flex h-full items-center justify-center text-center text-sm text-ink-muted">
                Списков нет
              </div>
            ) : (
              <Stagger className="flex flex-col gap-1.5">
                {s.lists.map((l) => (
                  <StaggerItem key={l.name}>
                    <ListRow
                      info={l}
                      selected={s.selected === l.name}
                      onSelect={() => s.select(l.name)}
                    />
                  </StaggerItem>
                ))}
              </Stagger>
            )}
          </div>
        </GlassPanel>

        {/* Правая колонка: редактор. */}
        <GlassPanel className="flex flex-col gap-3 overflow-hidden">
          {!s.selected ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
              <Icon.List size={30} />
              <div className="text-sm text-ink-soft">Выберите список слева</div>
            </div>
          ) : (
            <Editor
              key={s.selected}
              name={s.selected}
              info={current}
              draft={s.draft}
              entries={draftEntries}
              dirty={dirty}
              loading={s.loading}
              saving={s.saving}
              onChange={s.setDraft}
              onSave={s.save}
              onRevert={s.revert}
              onDelete={() => s.remove(s.selected)}
            />
          )}

          {s.error && (
            <div className="rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
              {s.error}
            </div>
          )}
        </GlassPanel>
      </div>
    </div>
  );
}

// ─── Строка списка в левой колонке ─────────────────────────────────────────

function ListRow({
  info,
  selected,
  onSelect,
}: {
  info: ListInfo;
  selected: boolean;
  onSelect: () => void;
}) {
  const isIpset = info.kind === "ipset";
  return (
    <button
      onClick={onSelect}
      className={[
        "no-drag flex w-full items-center justify-between gap-2 rounded-xl border px-3 py-2 text-left transition-[color,background-color,border-color,box-shadow,opacity]",
        selected
          ? "border-accent/50 bg-accent/15 shadow-glow"
          : "border-glass-border bg-white/5 hover:bg-white/10",
      ].join(" ")}
    >
      <div className="min-w-0">
        <div className="truncate text-sm font-medium text-ink">{label(info.name)}</div>
        <div className="mt-0.5 text-2xs text-ink-muted">
          {isIpset ? `${Math.round(info.bytes / 1024)} КБ` : `${info.entries} записей`}
        </div>
      </div>
      {isIpset && (
        <span className="shrink-0 rounded-md bg-warn/15 px-1.5 py-0.5 text-3xs font-semibold uppercase text-warn">
          ipset
        </span>
      )}
    </button>
  );
}

// ─── Форма создания нового списка ──────────────────────────────────────────

function NewListForm({
  onCreate,
  disabled,
  disabledHint,
}: {
  onCreate: (name: string) => Promise<boolean>;
  disabled: boolean;
  disabledHint: string;
}) {
  const [name, setName] = useState("");
  const [open, setOpen] = useState(false);

  const submit = async () => {
    const ok = await onCreate(name.trim());
    if (ok) {
      setName("");
      setOpen(false);
    }
  };

  if (!open) {
    return (
      <Button
        variant="ghost"
        disabled={disabled}
        onClick={() => setOpen(true)}
        className="w-full"
      >
        <span className="flex items-center justify-center gap-1.5">
          <Icon.Plus size={16} /> Новый список
        </span>
      </Button>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      <TextField
        value={name}
        placeholder="имя (латиница, цифры, _ -)"
        onChange={setName}
      />
      <div className="flex gap-2">
        <Button disabled={!name.trim()} onClick={submit} className="flex-1">
          Создать
        </Button>
        <Button
          variant="ghost"
          onClick={() => {
            setName("");
            setOpen(false);
          }}
        >
          Отмена
        </Button>
      </div>
      {disabled && disabledHint && (
        <p className="text-2xs text-warn">{disabledHint}</p>
      )}
    </div>
  );
}

// ─── Редактор списка ───────────────────────────────────────────────────────

function Editor({
  name,
  info,
  draft,
  entries,
  dirty,
  loading,
  saving,
  onChange,
  onSave,
  onRevert,
  onDelete,
}: {
  name: string;
  info: ListInfo | undefined;
  draft: string;
  entries: number;
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  onChange: (v: string) => void;
  onSave: () => void;
  onRevert: () => void;
  onDelete: () => void;
}) {
  const [confirmDel, setConfirmDel] = useState(false);
  const isIpset = info?.kind === "ipset";

  return (
    <>
      {/* Шапка редактора. */}
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate text-base font-semibold text-ink">{label(name)}</span>
            {dirty && (
              <span className="shrink-0 rounded-md bg-accent/20 px-1.5 py-0.5 text-3xs font-semibold uppercase text-accent">
                не сохранено
              </span>
            )}
          </div>
          <div className="mt-0.5 text-xs text-ink-muted">
            {name}.txt · {entries} записей
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button variant="ghost" disabled={!dirty || saving} onClick={onRevert}>
            Откатить
          </Button>
          <Button disabled={!dirty || saving} onClick={onSave}>
            {saving ? "Сохраняю…" : "Сохранить"}
          </Button>
        </div>
      </div>

      {/* Предупреждение для больших IP-наборов. */}
      {isIpset && (
        <div className="rounded-xl border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
          Большой список IP-диапазонов. Правьте только если понимаете формат
          CIDR — ошибки ломают обход. Обычно его менять не нужно.
        </div>
      )}

      {/* Текстовое поле. */}
      {loading ? (
        <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
          Загрузка содержимого…
        </div>
      ) : (
        <textarea
          value={draft}
          onChange={(e) => onChange(e.target.value)}
          spellCheck={false}
          className="no-drag flex-1 resize-none rounded-xl border border-glass-border bg-base-800/80 px-3 py-2.5 font-mono text-xs leading-relaxed text-ink outline-none transition-colors placeholder:text-ink-muted focus:border-accent/60"
          placeholder={"example.com\nsub.example.com\n# комментарий"}
        />
      )}

      {/* Удаление. */}
      <div className="flex items-center justify-between">
        <p className="text-2xs text-ink-soft">
          Изменения применяются при следующем запуске обхода.
        </p>
        <AnimatePresence mode="wait" initial={false}>
          {confirmDel ? (
            <motion.div
              key="confirm"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="flex items-center gap-2"
            >
              <span className="text-xs text-ink-soft">Удалить «{label(name)}»?</span>
              <Button variant="danger" onClick={onDelete}>
                Да
              </Button>
              <Button variant="ghost" onClick={() => setConfirmDel(false)}>
                Нет
              </Button>
            </motion.div>
          ) : (
            <motion.button
              key="del"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onClick={() => setConfirmDel(true)}
              className="no-drag flex items-center gap-1.5 rounded-lg px-2 py-1 text-xs text-ink-soft transition-colors hover:bg-danger/10 hover:text-danger"
            >
              <Icon.Trash size={14} /> Удалить список
            </motion.button>
          )}
        </AnimatePresence>
      </div>
    </>
  );
}

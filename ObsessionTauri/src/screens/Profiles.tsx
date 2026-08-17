import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useShallow } from "zustand/react/shallow";

import { useProfileStore } from "../store/profileStore";
import { type Profile } from "../lib/tauri";
import { GlassPanel } from "../design/components/GlassPanel";
import { Stagger, StaggerItem } from "../design/components/Stagger";
import { Button, SectionLabel, TextField } from "../design/components/atoms";
import { Icon } from "../design/components/icons";
import { spring } from "../design/tokens";

const CATEGORY_LABELS: Record<string, string> = {
  discord: "Discord",
  youtube_twitch: "YouTube / Twitch",
  gaming: "Gaming",
  universal: "Universal",
};

export function ProfilesScreen() {
  const s = useProfileStore(useShallow((state) => ({
    profiles: state.profiles,
    loaded: state.loaded,
    busy: state.busy,
    applyingId: state.applyingId,
    error: state.error,
    bootstrap: state.bootstrap,
    saveCurrent: state.saveCurrent,
    remove: state.remove,
    apply: state.apply,
  })));
  const [name, setName] = useState("");

  useEffect(() => {
    if (!s.loaded) s.bootstrap();
  }, []);

  const onSave = async () => {
    await s.saveCurrent(name);
    setName("");
  };

  return (
    <div className="flex h-full flex-col gap-4">
      {/* Заголовок. */}
      <StaggerItem standalone>
        <h1 className="font-display text-3xl font-semibold tracking-tight text-gradient">Профили</h1>
        <p className="text-sm text-ink-muted">
          Пресеты DPI + прокси + ИИ · применение в один клик
        </p>
      </StaggerItem>

      <div className="screen-split screen-split--start-360">
        {/* Сохранение текущего состояния. */}
        <GlassPanel className="flex flex-col gap-3">
          <SectionLabel>Новый профиль</SectionLabel>
          <p className="text-xs text-ink-muted">
            Снимок текущего выбора: категории и конфиги DPI, порт и домен
            Telegram-прокси, провайдер ИИ.
          </p>
          <TextField
            value={name}
            placeholder="Название профиля"
            onChange={setName}
          />
          <Button disabled={!name.trim() || s.busy} onClick={onSave}>
            <span className="flex items-center justify-center gap-1.5">
              <Icon.Layers size={16} /> Сохранить текущее
            </span>
          </Button>
          {s.error && (
            <div className="rounded-xl border border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
              {s.error}
            </div>
          )}
        </GlassPanel>

        {/* Список профилей. */}
        <GlassPanel scroll>
          {!s.loaded ? (
            <div className="flex h-full items-center justify-center text-sm text-ink-muted">
              Загрузка…
            </div>
          ) : s.profiles.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
              <Icon.Layers size={30} />
              <div className="text-sm text-ink-soft">Профилей пока нет</div>
              <div className="max-w-[260px] text-xs text-ink-muted">
                Настройте DPI и прокси, затем сохраните набор как профиль слева.
              </div>
            </div>
          ) : (
            <Stagger className="flex flex-col gap-3">
              <AnimatePresence initial={false}>
                {s.profiles.map((p) => (
                  <StaggerItem key={p.id}>
                    <ProfileCard
                      profile={p}
                      applying={s.applyingId === p.id}
                      busy={s.busy || s.applyingId !== ""}
                      onApply={() => s.apply(p)}
                      onDelete={() => s.remove(p.id)}
                    />
                  </StaggerItem>
                ))}
              </AnimatePresence>
            </Stagger>
          )}
        </GlassPanel>
      </div>
    </div>
  );
}

function ProfileCard({
  profile,
  applying,
  busy,
  onApply,
  onDelete,
}: {
  profile: Profile;
  applying: boolean;
  busy: boolean;
  onApply: () => void;
  onDelete: () => void;
}) {
  const [confirm, setConfirm] = useState(false);

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, y: -6 }}
      transition={spring.expand}
      className="rounded-xl border border-glass-border bg-white/5 p-4"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-base font-semibold text-ink">{profile.name}</div>
          <div className="mt-0.5 text-xs text-ink-muted">
            Прокси :{profile.proxy_port}
            {profile.fake_tls_domain ? ` · ${profile.fake_tls_domain}` : ""} · ИИ{" "}
            {profile.ai_provider}
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          <Button disabled={busy} onClick={onApply}>
            {applying ? "Применяю…" : "Применить"}
          </Button>
          {confirm ? (
            <Button variant="danger" disabled={busy} onClick={onDelete}>
              Точно?
            </Button>
          ) : (
            <Button variant="ghost" disabled={busy} onClick={() => setConfirm(true)}>
              Удалить
            </Button>
          )}
        </div>
      </div>

      {/* Категории DPI. */}
      {profile.selected_categories.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {profile.selected_categories.map((c) => (
            <span
              key={c}
              className="rounded-md bg-white/8 px-2 py-0.5 text-2xs font-medium text-ink-soft"
            >
              {CATEGORY_LABELS[c] ?? c}
            </span>
          ))}
        </div>
      )}
    </motion.div>
  );
}

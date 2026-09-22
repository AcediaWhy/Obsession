import { invoke } from "@tauri-apps/api/core";

const preview = import.meta.env.DEV && new URLSearchParams(location.search).has("preview");
try {
  const uninstall = preview
    ? new URLSearchParams(location.search).get("preview") === "uninstall"
    : await invoke<boolean>("uninstall_mode");
  if (uninstall) await import("./uninstall");
  else await import("./main");
} catch {
  document.getElementById("app")!.textContent = "Не удалось открыть Obsession Setup. Закрой окно и попробуй снова.";
}

export function dpiTestBlockedReason(state: {
  available: boolean; transitioning: boolean; adaptiveBusy: boolean;
  active: boolean; testing: boolean; categories: number;
}): string {
  if (!state.available) return "Тест недоступен: нет связи с системной службой DPI.";
  if (state.transitioning) return "Дождитесь завершения переключения защиты.";
  if (state.adaptiveBusy) return "Дождитесь завершения адаптивного поиска.";
  if (state.active && !state.testing) return "Выключите защиту перед проверкой конфигов.";
  if (!state.categories) return "Выберите категории для проверки.";
  return "";
}

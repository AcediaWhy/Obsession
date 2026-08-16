/**
 * Keep the native Telegram URI in the QR payload so a phone can hand it
 * directly to the installed Telegram client instead of opening t.me first.
 */
export function resolveTelegramQrLink(rawLink: string | null): string | null {
  return rawLink;
}

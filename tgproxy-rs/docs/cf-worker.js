// Archived Cloudflare Worker scaffold for Obsession Telegram Proxy.
//
// Investigation on 2026-08-17 showed that Telegram rejects MTProto/WebSocket
// sessions originating from Cloudflare Workers egress. Keeping the old relay,
// probe, and debug endpoints deployable would expose an unauthenticated public
// proxy without providing a working fallback. This scaffold is intentionally
// fail-closed until Telegram's egress policy changes and an authenticated design
// is reviewed.

export default {
	async fetch() {
		return new Response(
			"Obsession Telegram relay scaffold is disabled: Cloudflare Workers egress is not accepted by Telegram.",
			{
				status: 410,
				headers: {
					"cache-control": "no-store",
					"content-type": "text/plain; charset=utf-8",
				},
			},
		);
	},
};

// Cloudflare Worker для личного relay-фолбэка tg_ws_proxy (Obsession).
//
// ВАЖНО (результаты расследования 2026-08-17): Telegram отвергает
// MTProto-сессии, идущие из egress Cloudflare Workers, независимо от
// содержимого (проверено побайтово-идентичными init с рабочего IP:
// fetch-WebSocket -> close 1000/-404; сырой TLS-сокет -> HTTP 400 на
// апгрейде). Рекомендуемый путь к DC — прямое подключение прокси; этот
// воркер оставлен как каркас на случай изменения политики Telegram.
//
// Эндпоинты:
//   GET /apiws?dc=<N>&m=<0|1>  — relay к kws{dc}[-1].web.telegram.org
//   GET /debug                 — последняя ошибка сессии (per-isolate)
//   GET /probe?dc=<N>          — самотест: коннект к своему DC + протокол

const DC_HOST_RE = /^kws\d{1,5}(-1)?\.web\.telegram\.org$/;

let lastDebug = "no sessions yet";

function note(text) {
	lastDebug = `${new Date().toISOString()} ${text}`;
}

function dcHost(dc, media) {
	const host = `kws${dc}${media ? "-1" : ""}.web.telegram.org`;
	if (!DC_HOST_RE.test(host)) {
		return null;
	}
	return host;
}

export default {
	async fetch(request) {
		const url = new URL(request.url);

		if (url.pathname === "/debug") {
			return new Response(lastDebug, { status: 200 });
		}

		if (url.pathname === "/probe") {
			const dc = url.searchParams.get("dc") || "2";
			const host = dcHost(dc, false);
			if (!host) {
				return new Response("Bad dc", { status: 400 });
			}
			const events = [];
			const log = (text) => events.push(`${new Date().toISOString().slice(11, 23)} ${text}`);
			let response;
			try {
				response = await fetch(`https://${host}/apiws`, {
					headers: {
						Upgrade: "websocket",
						Connection: "Upgrade",
						"Sec-WebSocket-Protocol": "binary",
					},
				});
				log(`upstream ${host}: HTTP ${response.status}`);
			} catch (error) {
				log(`upstream ${host} threw: ${error}`);
			}
			if (response?.webSocket) {
				const ws = response.webSocket;
				ws.accept();
				const outcome = await new Promise((resolve) => {
					let received = 0;
					ws.addEventListener("message", (event) => {
						received += event.data?.byteLength ?? 0;
						if (received >= 64) {
							resolve(`data flowing (${received}B)`);
						}
					});
					ws.addEventListener("close", (event) => {
						resolve(`closed code=${event.code} reason='${event.reason}' received=${received}B`);
					});
					ws.addEventListener("error", () => resolve(`error, received=${received}B`));
					try {
						ws.send(new Uint8Array(64));
					} catch (error) {
						resolve(`send threw: ${error}`);
					}
					setTimeout(() => resolve(`timeout 5s, received=${received}B`), 5000);
				});
				log(`outcome: ${outcome}`);
				try {
					ws.close();
				} catch {}
			}
			return new Response(events.join("\n"), {
				status: 200,
				headers: { "content-type": "text/plain; charset=utf-8" },
			});
		}

		if ((request.headers.get("Upgrade") || "").toLowerCase() !== "websocket") {
			return new Response("Expected websocket", { status: 426 });
		}
		if (url.pathname !== "/apiws") {
			return new Response("Not found", { status: 404 });
		}

		const dc = url.searchParams.get("dc") || "2";
		const media = url.searchParams.get("m") === "1";
		const host = dcHost(dc, media);
		if (!host) {
			return new Response("Bad dc", { status: 400 });
		}

		let upstreamResponse;
		try {
			upstreamResponse = await fetch(`https://${host}/apiws`, {
				headers: {
					Upgrade: "websocket",
					Connection: "Upgrade",
					"Sec-WebSocket-Protocol": "binary",
				},
			});
		} catch (error) {
			note(`upstream fetch to ${host} threw: ${error}`);
			return new Response(`Upstream failed: ${error}`, { status: 502 });
		}

		const upstream = upstreamResponse.webSocket;
		if (!upstream) {
			note(`upstream ${host} no websocket, status ${upstreamResponse.status}`);
			return new Response("Upstream is not websocket", { status: 502 });
		}
		upstream.accept();
		note(`upstream ${host} websocket established`);

		const pair = new WebSocketPair();
		const client = pair[0];
		const server = pair[1];
		server.accept();

		let forwarded = 0;
		let received = 0;

		server.addEventListener("message", (event) => {
			try {
				upstream.send(event.data);
				forwarded++;
			} catch (error) {
				note(`client->upstream send #${forwarded} threw: ${error}`);
				try {
					server.close(1011, "upstream send failed");
				} catch {}
			}
		});
		upstream.addEventListener("message", (event) => {
			try {
				server.send(event.data);
				received++;
			} catch (error) {
				note(`upstream->client send #${received} threw: ${error}`);
				try {
					upstream.close(1011, "client send failed");
				} catch {}
			}
		});
		server.addEventListener("close", (event) => {
			note(`client closed (code ${event.code}, ${forwarded} sent, ${received} recv)`);
			try {
				upstream.close(event.code, event.reason);
			} catch {}
		});
		upstream.addEventListener("close", (event) => {
			note(`upstream closed (code ${event.code}, ${forwarded} sent, ${received} recv)`);
			try {
				server.close(event.code, event.reason);
			} catch {}
		});

		return new Response(null, { status: 101, webSocket: client });
	},
};

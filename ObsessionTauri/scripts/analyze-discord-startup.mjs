import fs from 'node:fs';
import crypto from 'node:crypto';
import { pathToFileURL } from 'node:url';

// Never emit arbitrary params, URLs, headers, handshake bytes or clientInfo.
const publicDiscordHost = value => {
  const match = String(value ?? '').match(/(?:^|:\/\/)((?:[a-z0-9-]+\.)*(?:discord\.com|discord\.gg|discordapp\.com|discordapp\.net))(?=[:/\s]|$)/i);
  return match?.[1]?.toLowerCase() ?? null;
};

export function readNetLog(text) {
  try { return { complete: true, data: JSON.parse(text) }; } catch {}
  const marker = /"events"\s*:\s*\[/.exec(text);
  if (!marker) throw new Error('No recoverable NetLog event array');
  const start = marker.index + marker[0].length;
  const data = JSON.parse(text.slice(0, start) + ']}');
  data.events = [];
  // Chromium writes one event per line. Preserve only complete JSON records.
  for (const line of text.slice(start).split(/\r?\n/)) {
    const event = line.trim().replace(/,$/, '');
    if (!event) continue;
    if (!event.startsWith('{')) break;
    try { data.events.push(JSON.parse(event)); } catch { break; }
  }
  return { complete: false, data };
}

export function clientHelloSummary(encoded) {
  const bytes = Buffer.from(String(encoded ?? ''), 'base64');
  const read16 = offset => bytes.readUInt16BE(offset);
  if (bytes.length < 40 || bytes[0] !== 1 || bytes.readUIntBE(1, 3) !== bytes.length - 4) return null;
  try {
    let pos = 4 + 2 + 32;
    pos += 1 + bytes[pos];
    pos += 2 + read16(pos);
    pos += 1 + bytes[pos];
    const end = pos + 2 + read16(pos);
    pos += 2;
    if (end !== bytes.length) return null;
    const result = { handshakeBytes: bytes.length, sni: null, sniOffset: null, sniEndOffset: null, sessionTicketBytes: null, alpn: [], extensionTypes: [], keyShares: [] };
    while (pos + 4 <= end) {
      const type = read16(pos), length = read16(pos + 2), payload = pos + 4;
      pos = payload + length;
      if (pos > end) return null;
      result.extensionTypes.push(type);
      if (type === 0 && length >= 5 && bytes[payload + 2] === 0) {
        if (payload + 5 + read16(payload + 3) > pos) return null;
        result.sni = publicDiscordHost(bytes.subarray(payload + 5, payload + 5 + read16(payload + 3)).toString('ascii'));
        if (result.sni) {
          result.sniOffset = payload + 5;
          result.sniEndOffset = result.sniOffset + read16(payload + 3);
        }
      }
      if (type === 35) result.sessionTicketBytes = length;
      if (type === 16) for (let at = payload + 2; at < pos;) {
        const size = bytes[at++], protocol = bytes.subarray(at, at + size).toString('ascii');
        at += size;
        if (['h2', 'http/1.1'].includes(protocol)) result.alpn.push(protocol);
      }
      if (type === 51) for (let at = payload + 2; at + 4 <= pos;) {
        const group = read16(at), keyBytes = read16(at + 2);
        at += 4 + keyBytes;
        if (at > pos) return null;
        result.keyShares.push({ group, keyBytes });
      }
    }
    return pos === end ? result : null;
  } catch { return null; }
}

export function summarizeNetLog(text) {
  const { complete, data } = readNetLog(text);
  const typeNames = Object.fromEntries(Object.entries(data.constants?.logEventTypes ?? {}).map(([name, id]) => [id, name]));
  const events = data.events ?? [], offset = Number(data.constants?.timeTickOffset);
  // A fast client run through a local proxy is not evidence of direct DPI
  // bypass. Retain classification counts, never proxy endpoints or passwords.
  const proxyResolution = { direct: 0, proxied: 0, unknown: 0 };
  for (const row of events) {
    if (typeNames[row.type] !== 'PROXY_RESOLUTION_SERVICE_RESOLVED_PROXY_LIST') continue;
    const value = row.params?.proxy_chain ?? row.params?.proxy_list ?? row.params?.proxy_info;
    const text = typeof value === 'string' ? value.trim() : '';
    const protocols = [...text.matchAll(/\b(direct|https?|socks[45]?):\/\//gi)].map(match => match[1].toLowerCase());
    if (/^DIRECT$/i.test(text) || (protocols.length > 0 && protocols.every(protocol => protocol === 'direct'))) proxyResolution.direct++;
    else if (protocols.some(protocol => protocol !== 'direct') || /\b(PROXY|HTTPS|SOCKS[45]?)\s+\S+/i.test(text)) proxyResolution.proxied++;
    else proxyResolution.unknown++;
  }
  const time = row => Number(row.time) + offset;
  const sources = new Map();
  for (const row of events) {
    if (!sources.has(row.source.id)) sources.set(row.source.id, []);
    sources.get(row.source.id).push(row);
  }
  const tls = [], requests = [];
  for (const [source, rows] of sources) {
    const start = rows.find(row => typeNames[row.type] === 'URL_REQUEST_START_JOB' && row.phase === 1);
    const host = publicDiscordHost(start?.params?.url);
    let url;
    try { url = new URL(start?.params?.url); } catch {}
    if (host && url?.hostname === host && ['https:', 'wss:'].includes(url.protocol)) {
      const ends = rows.filter(row => typeNames[row.type] === 'URL_REQUEST' && row.phase === 2);
      const finish = ends.at(-1);
      const failures = rows.filter(row => Number.isInteger(row.params?.net_error) && row.params.net_error < 0);
      const response = rows.find(row => typeNames[row.type] === 'HTTP_TRANSACTION_READ_RESPONSE_HEADERS');
      const headers = response?.params?.headers;
      const statusLine = (Array.isArray(headers) ? headers : typeof headers === 'string' ? headers.split(/\r?\n/) : [])
        .find(line => /^(?:HTTP\/\S+\s+|:status:\s*)\d{3}/i.test(String(line)));
      const status = statusLine ? /(?:HTTP\/\S+\s+|:status:\s*)(\d{3})/i.exec(statusLine)?.[1] : null;
      const bytes = rows.filter(row => typeNames[row.type] === 'URL_REQUEST_JOB_BYTES_READ')
        .reduce((sum, row) => sum + (Number.isSafeInteger(row.params?.byte_count) && row.params.byte_count > 0 ? row.params.byte_count : 0), 0);
      // Retain a coarse path category; user IDs, asset names and query strings
      // never leave the raw local capture through this report.
      const kind = url.pathname.startsWith('/assets/') ? 'assets' : url.pathname.startsWith('/api/') ? 'api'
        : host.includes('updates') || host.includes('.dl') ? 'updater' : 'other';
      requests.push({ host, kind, status: status ? Number(status) : null,
        bytes, elapsedMs: finish ? Number(finish.time) - Number(start.time) : null,
        complete: !!finish, netError: failures.at(-1)?.params.net_error ?? null });
    }
    const hello = rows.find(row => typeNames[row.type] === 'SSL_HANDSHAKE_MESSAGE_SENT' && row.params?.type === 1);
    const metadata = clientHelloSummary(hello?.params?.bytes);
    if (!metadata?.sni) continue;
    const begin = name => rows.find(row => typeNames[row.type] === name && row.phase === 1);
    const end = name => rows.find(row => typeNames[row.type] === name && row.phase === 2);
    const tcpStart = begin('TCP_CONNECT'), tcpEnd = end('TCP_CONNECT');
    const sslStart = begin('SSL_CONNECT'), sslEnd = end('SSL_CONNECT');
    const addresses = tcpStart?.params?.address_list ?? [];
    const error = rows.find(row => typeNames[row.type] === 'SSL_HANDSHAKE_ERROR');
    tls.push({ source, ...metadata,
      remoteAddresses: addresses.filter(value => /^(?:\d{1,3}\.){3}\d{1,3}:443$/.test(value)),
      tcpMs: tcpStart && tcpEnd ? Number(tcpEnd.time) - Number(tcpStart.time) : null,
      tlsMs: sslStart && sslEnd ? Number(sslEnd.time) - Number(sslStart.time) : null,
      tlsStartUtc: sslStart ? new Date(time(sslStart)).toISOString() : null,
      tlsEndUtc: sslEnd ? new Date(time(sslEnd)).toISOString() : null,
      resumed: typeof sslEnd?.params?.is_resumed === 'boolean' ? sslEnd.params.is_resumed : null,
      encryptedClientHelloAccepted: typeof sslEnd?.params?.encrypted_client_hello === 'boolean' ? sslEnd.params.encrypted_client_hello : null,
      netError: Number.isInteger(error?.params?.net_error) ? error.params.net_error : null,
      serverHelloObserved: rows.some(row => typeNames[row.type] === 'SSL_HANDSHAKE_MESSAGE_RECEIVED' && row.params?.type === 2)
    });
  }
  return { schema: 'discord_netlog_safe_v1', complete, recoveredEvents: events.length,
    firstEventUtc: events.length ? new Date(time(events[0])).toISOString() : null,
    lastEventUtc: events.length ? new Date(time(events.at(-1))).toISOString() : null,
    proxyResolution, tls, requests };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const input = process.argv[2], output = process.argv[3];
    if (!input) throw new Error('Missing input');
    const text = fs.readFileSync(input, 'utf8').replace(/^\uFEFF/, '');
    const report = { ...summarizeNetLog(text), sourceSha256: crypto.createHash('sha256').update(fs.readFileSync(input)).digest('hex') };
    const safe = JSON.stringify(report, null, 2) + '\n';
    if (output) fs.writeFileSync(output, safe);
    process.stdout.write(safe);
  } catch {
    process.stderr.write('Unable to analyze NetLog; raw contents and parser details omitted.\n');
    process.exitCode = 1;
  }
}

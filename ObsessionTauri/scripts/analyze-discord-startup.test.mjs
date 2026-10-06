import assert from 'node:assert/strict';
import test from 'node:test';
import { clientHelloSummary, readNetLog, summarizeNetLog } from './analyze-discord-startup.mjs';

const u16 = n => { const out = Buffer.alloc(2); out.writeUInt16BE(n); return out; };
function hello(host) {
  const hostname = Buffer.from(host);
  const sni = Buffer.concat([u16(hostname.length + 3), Buffer.from([0]), u16(hostname.length), hostname]);
  const extension = Buffer.concat([u16(0), u16(sni.length), sni]);
  const body = Buffer.concat([u16(0x0303), Buffer.alloc(32), Buffer.from([0]), u16(2), u16(0x1301), Buffer.from([1, 0]), u16(extension.length), extension]);
  const header = Buffer.alloc(4); header[0] = 1; header.writeUIntBE(body.length, 1, 3);
  return Buffer.concat([header, body]).toString('base64');
}
function log(events) {
  return { constants: {timeTickOffset: '1791000000000', clientInfo: {secret: 'SECRET_ACCOUNT'}, logEventTypes: { SSL_HANDSHAKE_MESSAGE_SENT: 1, SSL_CONNECT: 2, SSL_HANDSHAKE_ERROR: 3 }}, events };
}
const row = (type, phase, params = {}, time = '100') => ({ source: { id: 3 }, type, phase, time, params });

test('sanitized result excludes account data, query strings, headers and raw handshake', () => {
  const encoded = hello('cdn.discordapp.com');
  const input = log([
    row(2, 1, {headers: ['Authorization: SECRET_ACCOUNT'], url: 'https://cdn.discordapp.com/private/SECRET_ACCOUNT'}),
    row(1, 0, {type: 1, bytes: encoded, token: 'SECRET_ACCOUNT'}),
    row(3, 0, {net_error: -101, session_id: 'SECRET_ACCOUNT'}, '19100'),
    row(2, 2, {}, '19100')
  ]);
  const report = summarizeNetLog(JSON.stringify(input)), output = JSON.stringify(report);
  assert.equal(report.tls[0].tlsMs, 19000);
  assert.equal(report.tls[0].sni, 'cdn.discordapp.com');
  assert.equal(report.tls[0].netError, -101);
  for (const forbidden of ['SECRET_ACCOUNT', 'Authorization', encoded, '/private/']) assert.equal(output.includes(forbidden), false);
});
test('interrupted NetLog recovers complete events and remains marked partial', () => {
  const input = log([]);
  const prefix = JSON.stringify(input).replace(/\[\]\}$/, '[\n');
  const text = prefix + JSON.stringify(row(1, 0, {type: 1, bytes: hello('cdn.discordapp.com')})) + ',\n{"source":';
  const parsed = readNetLog(text);
  assert.equal(parsed.complete, false);
  assert.equal(parsed.data.events.length, 1);
  const report = summarizeNetLog(text);
  assert.equal(report.complete, false);
  assert.equal(report.tls[0].tlsMs, null);
});
test('truncated handshake cannot invent TLS/SNI evidence', () => {
  const bytes = Buffer.from(hello('cdn.discordapp.com'), 'base64');
  assert.equal(clientHelloSummary(bytes.subarray(0, -1).toString('base64')), null);
  assert.equal(clientHelloSummary('invalid'), null);
});
test('unrelated private host is excluded from the report', () => {
  const report = summarizeNetLog(JSON.stringify(log([row(1, 0, {type: 1, bytes: hello('secret.example')})])));
  assert.deepEqual(report.tls, []);
  assert.equal(JSON.stringify(report).includes('secret.example'), false);
});
test('SNI offsets identify hostname bytes and malformed extension bounds are rejected', () => {
  const bytes = Buffer.from(hello('cdn.discordapp.com'), 'base64');
  const summary = clientHelloSummary(bytes.toString('base64'));
  assert.equal(summary.sniOffset, bytes.indexOf(Buffer.from('cdn.discordapp.com')));
  assert.equal(summary.sniEndOffset-summary.sniOffset, 'cdn.discordapp.com'.length);
  bytes.writeUInt16BE(10000, summary.sniOffset-2);
  assert.equal(clientHelloSummary(bytes.toString('base64')), null);
});

test('request diagnosis retains stalls and transfer sizes without account URLs or headers', () => {
  const input = log([]);
  Object.assign(input.constants.logEventTypes, {URL_REQUEST_START_JOB:4, URL_REQUEST:5,
    URL_REQUEST_JOB_BYTES_READ:6, HTTP_TRANSACTION_READ_RESPONSE_HEADERS:7});
  input.events = [
    row(4, 1, {url:'https://discord.com/api/v9/users/SECRET_ACCOUNT?token=PRIVATE_TOKEN'}, '100'),
    row(7, 0, {headers:['HTTP/2 200', 'Set-Cookie: PRIVATE_COOKIE']}, '200'),
    row(6, 0, {byte_count:4096, bytes:'PRIVATE_BODY'}, '300'),
    row(5, 2, {net_error:-101}, '15100'),
    {...row(4, 1, {url:'https://secret.invalid/PRIVATE_ACCOUNT'}), source:{id:4}}
  ];
  const report = summarizeNetLog(JSON.stringify(input));
  assert.deepEqual(report.requests, [{host:'discord.com',kind:'api',status:200,bytes:4096,elapsedMs:15000,complete:true,netError:-101}]);
  for (const value of ['SECRET_ACCOUNT','PRIVATE_TOKEN','PRIVATE_COOKIE','PRIVATE_BODY','secret.invalid','/users/']) {
    assert.equal(JSON.stringify(report).includes(value), false);
  }
  input.events = input.events.slice(0,3);
  const pending = summarizeNetLog(JSON.stringify(input)).requests[0];
  assert.equal(pending.complete,false);
  assert.equal(pending.elapsedMs,null);
});

test('direct and proxied runs stay distinct without exposing proxy configuration', () => {
  const input = log([]);
  input.constants.logEventTypes.PROXY_RESOLUTION_SERVICE_RESOLVED_PROXY_LIST = 9;
  input.events = [row(9, 0, {proxy_chain:'[direct://]'}), row(9, 0, {proxy_list:'DIRECT'}),
    row(9, 0, {proxy_chain:'[http://PRIVATE_PROXY:8080]'}),
    row(9, 0, {proxy_list:'PROXY SECRET_HOST:1080; DIRECT'}),
    row(9, 0, {proxy_chain:'unsupported'}), row(9, 0, {proxy_info:'DIRECT'}),
    row(9, 0, {proxy_info:'PROXY PRIVATE_ENDPOINT:1080'})];
  const report = summarizeNetLog(JSON.stringify(input));
  assert.deepEqual(report.proxyResolution, {direct:3,proxied:3,unknown:1});
  for (const value of ['PRIVATE_PROXY','SECRET_HOST','PRIVATE_ENDPOINT','8080','1080','unsupported']) assert.equal(JSON.stringify(report).includes(value),false);
});

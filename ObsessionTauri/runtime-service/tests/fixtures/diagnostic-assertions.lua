local ctx = {data="fixture_packet_bytes_never_log"}
local desync = {arg={profile="adaptive-discord-test"}, l7payload="tls_client_hello", outgoing=true}
assert(obsession_trace(ctx, desync) == VERDICT_PASS)
local verdict = diagnostic_pack(fake(ctx, desync))
assert(verdict.n == 4 and verdict[1] == nil and verdict[2] == 5 and verdict[3] == nil and verdict[4] == "verdict_tail")
local args = diagnostic_test.calls[1]
assert(args.n == 3 and args[1] == ctx.data and args[2] == nil and args[3] == desync)
local sent = diagnostic_pack(rawsend("fail", nil, desync))
assert(sent.n == 3 and sent[1] == false and sent[2] == nil and sent[3] == "tail")
for _, action in ipairs({function() multidisorder(ctx, desync) end,
    function() multidisorder_legacy(ctx, desync) end, function() rawsend("throw") end}) do
    local ok, err = pcall(action)
    assert(not ok and err == diagnostic_test.error)
end
local nil_verdict = diagnostic_pack(pass(ctx, desync))
assert(nil_verdict.n == 1 and nil_verdict[1] == nil)
local output = table.concat(diagnostic_test.lines)
assert(output:find("OBS_DIAG|ready|v1", 1, true))
assert(output:find("OBS_DIAG|profile|adaptive-discord-test|tls_client_hello|out", 1, true))
assert(output:find("OBS_DIAG|send|adaptive-discord-test|fake|rawsend|ok", 1, true))
assert(output:find("OBS_DIAG|send|adaptive-discord-test|none|rawsend|failed", 1, true))
assert(output:find("OBS_DIAG|lua_error|adaptive-discord-test|multidisorder", 1, true))
assert(output:find("OBS_DIAG|lua_error|adaptive-discord-test|multidisorder_legacy", 1, true))
assert(not output:find(ctx.data, 1, true))
-- Real API 6 packet metadata shapes, with inert native sends. Routing evidence
-- belongs to the exact outgoing ClientHello socket and survives nil returns.
local routed = {arg={profile="api-parent",routing=true},outgoing=true,l7payload="tls_client_hello",
    dis={ip={ip_src=string.char(192,0,2,1),ip_dst=string.char(203,0,113,1)},tcp={th_sport=50001,th_dport=443}}}
obsession_trace(ctx,routed); assert(select(2,fake(ctx,routed))==5)
local line_count=#diagnostic_test.lines
obsession_trace(ctx,routed); fake(ctx,routed)
assert(#diagnostic_test.lines==line_count) -- retransmits do not flood the buffer
local routed_output=table.concat(diagnostic_test.lines)
assert(routed_output:find("OBS_DIAG|flow_route|v1|api-parent|c0000201|50001|cb007101|443|selected|none",1,true))
assert(routed_output:find("OBS_DIAG|flow_route|v1|api-parent|c0000201|50001|cb007101|443|action_ok|fake",1,true))
assert(routed_output:find("OBS_DIAG|flow_route|v1|api-parent|c0000201|50001|cb007101|443|send_ok|fake",1,true))
assert(not routed_output:find("fixture_packet_bytes_never_log",1,true))
local bad_ctx={data="fail"}; fake(bad_ctx,routed)
assert(table.concat(diagnostic_test.lines):find("|send_error|fake",1,true))
local before_inbound=#diagnostic_test.lines
routed.outgoing=false; obsession_trace(ctx,routed); fake(ctx,routed)
assert(#diagnostic_test.lines==before_inbound)
routed.outgoing=true; routed.l7payload="unknown"; obsession_trace(ctx,routed); fake(ctx,routed)
assert(#diagnostic_test.lines==before_inbound)
-- Restore the ordinary observer contract for the remaining assertions.
obsession_trace(ctx,desync)
-- Broken logging must not change the packet function's result.
io.stdout.write = function() error("fixture logging failure") end
assert(select(2, fake(ctx, desync)) == 5)
-- Once observation is exhausted, forwarding continues without logging.
for i=1,2000 do assert(select(2, fake(ctx, desync)) == 5) end
io.stdout = diagnostic_test.stdout
print("OBS_DIAGNOSTIC_CONTRACT_OK")

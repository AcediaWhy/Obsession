-- Explicit experimental profile for Zapret2 Lua API 6.
-- Load with zapret-lib.lua and zapret-antidpi.lua (either order after lib).
-- Invocation:
--   --lua-desync=obsession_clienthello:payload=tls_client_hello
-- No selection, timers, history or globally retained packet contents.
-- The fake is a clone of THIS connection's complete ClientHello. Its TCP
-- sequence interval is wholly before the original bytes, not a replacement
-- for them. Whether a particular DPI still inspects that interval is unknown.
-- Timestamps, TTL, checksum and ACK are deliberately left untouched: a TS
-- option on one packet alone is not proof that PAWS was negotiated.
local MAX_HELLO = 8192
local FAKE_SEQ_DELTA = -16384
local FAKE_HOST = "www.google.com"

local function complete_hello(data)
    if type(data) ~= "string" or #data < 9 or #data > MAX_HELLO then return false end
    if data:byte(1) ~= 22 or data:byte(2) ~= 3 or data:byte(6) ~= 1 then return false end
    local record_size = data:byte(4) * 256 + data:byte(5)
    local handshake_size = data:byte(7) * 65536 + data:byte(8) * 256 + data:byte(9)
    -- Native clone/parser supports the single-record ClientHello. Other TLS
    -- record layouts and trailing records pass unchanged in this profile.
    return record_size == #data - 5 and handshake_size == record_size - 4
end

local function call_view(desync, arg)
    -- Keep the real packet, reassembly metadata, track and instance identity.
    -- Only per-call arguments/blob fields belong to this temporary table.
    local view = {}
    for key, value in pairs(desync) do view[key] = value end
    view.arg = arg
    return view
end

function obsession_clienthello(ctx, desync)
    if type(desync) ~= "table" or not desync.outgoing or desync.l7payload ~= "tls_client_hello"
        or type(desync.dis) ~= "table" or type(desync.dis.tcp) ~= "table"
        or type(desync.track) ~= "table" or type(desync.track.lua_state) ~= "table"
        or type(desync.func_instance) ~= "string" or type(desync.tcp_mss) ~= "number"
        or desync.tcp_mss < 128 or type(desync.dis.tcp.th_seq) ~= "number"
        or type(desync.dis.tcp.th_flags) ~= "number"
        or bitand(desync.dis.tcp.th_flags, TH_SYN + TH_FIN + TH_RST + TH_URG) ~= 0
        or bitand(desync.dis.tcp.th_flags, TH_ACK) == 0 then
        return VERDICT_PASS
    end

    local split = call_view(desync, {payload = "tls_client_hello", pos = "2,midsld", dir = "out"})
    if not replay_first(desync) then
        -- Upstream marks the replay only after EVERY real segment was sent.
        -- Failed/partial sends therefore keep all original replay pieces.
        return multisplit(ctx, split) or VERDICT_PASS
    end
    local data = desync.reasm_data or desync.dis.payload
    if not complete_hello(data) then return VERDICT_PASS end
    -- A valid native SNI marker also proves the native parser recognizes the
    -- complete hello. Missing SNI/ECH-only or malformed extensions pass.
    local positions = resolve_multi_pos(data, "tls_client_hello", "midsld")
    if not positions or #positions ~= 1 or positions[1] <= 1 or positions[1] >= #data then
        return VERDICT_PASS
    end

    local key = desync.func_instance .. "_obsession_clienthello_fake"
    if not desync.track.lua_state[key] then
        local clone = call_view(desync, {blob = "obsession_hello_fake", sni_del = "",
            sni_first = FAKE_HOST, dir = "out"})
        -- The upstream TLS parser can decline malformed extensions. Preserve
        -- the original also if reconstruction raises rather than returning nil.
        local cloned = pcall(tls_client_hello_clone, ctx, clone)
        local fake_data = clone.obsession_hello_fake
        -- Do not transmit an unbounded clone or a clone that retained the
        -- target name because the native transformation could not apply.
        if not cloned or not complete_hello(fake_data) or fake_data == data then return VERDICT_PASS end
        -- At most one fake attempt per connection, even after retransmit or
        -- raw-send failure. Never retain the ClientHello in persistent state.
        desync.track.lua_state[key] = true
        local injection = call_view(desync, {blob = "obsession_hello_fake", optional = "",
            payload = "tls_client_hello", tcp_seq = tostring(FAKE_SEQ_DELTA), dir = "out"})
        injection.obsession_hello_fake = fake_data
        if not pcall(fake, ctx, injection) then return VERDICT_PASS end
    end
    -- Native segmentation is MSS-aware, preserves data and TCP timestamps,
    -- sends in original order, and drops replay only on complete success.
    return multisplit(ctx, split) or VERDICT_PASS
end

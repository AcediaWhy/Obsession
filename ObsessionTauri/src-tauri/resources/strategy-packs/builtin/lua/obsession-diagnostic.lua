-- Bounded metadata observer for an explicit protected diagnostic launch.
-- Never logs packet bytes, hostnames, URLs, blobs or connection contents.
-- Wrappers forward every argument and return value and rethrow Lua errors.
local prefix = "OBS_DIAG|"
local deadline = os.time() + 120
local remaining = 512
local profile, current_function = "unknown", "none"
local routing_only, current_flow = false, nil
local route_seen = {}
-- The shipped LuaJIT uses the Lua 5.1 API: table.pack/table.unpack are absent.
-- Count explicitly so nil arguments and nil/multiple returns remain intact.
local function pack(...) return {n=select("#", ...), ...} end
local unpack_values = unpack or table.unpack
local function token(value)
    return tostring(value):gsub("[^%w_%.%-]", "_"):sub(1, 96)
end
local function emit(event, ...)
    -- The flow executor records one bounded receipt with exact socket identity.
    -- Avoid flooding the service buffer with an event for every segment send.
    if _G.obsession_flow_receipt_active or routing_only then return end
    if remaining <= 0 or os.time() > deadline then return end
    local fields = {prefix .. token(event)}
    local values = pack(...)
    for i = 1, values.n do fields[#fields + 1] = token(values[i]) end
    -- A logging failure must never affect the engine's packet verdict.
    pcall(function()
        io.stdout:write(table.concat(fields, "|") .. "\n")
        io.stdout:flush()
    end)
    remaining = remaining - 1
    if remaining == 0 then
        pcall(function() io.stdout:write(prefix .. "limit\n"); io.stdout:flush() end)
    end
end

-- Exact socket metadata, only for outgoing ClientHello packets. Packed IPs
-- are hex encoded: no SNI, payload, user session or packet bytes are retained.
local function flow_identity(desync)
    if not desync.outgoing or desync.l7payload ~= "tls_client_hello" then return nil end
    local dis = desync.dis
    if not dis or not dis.tcp then return nil end
    local source = dis.ip and dis.ip.ip_src or dis.ip6 and dis.ip6.ip6_src
    local destination = dis.ip and dis.ip.ip_dst or dis.ip6 and dis.ip6.ip6_dst
    if type(source) ~= "string" or type(destination) ~= "string"
        or (#source ~= 4 and #source ~= 16) or #source ~= #destination then return nil end
    local function hex(value) return (value:gsub(".", function(byte) return string.format("%02x", string.byte(byte)) end)) end
    return {hex(source), dis.tcp.th_sport, hex(destination), dis.tcp.th_dport}
end
local function route(event, action)
    if not current_flow or remaining <= 0 or os.time() > deadline then return end
    local fields = {prefix .. "flow_route", "v1", profile}
    for _, value in ipairs(current_flow) do fields[#fields + 1] = token(value) end
    fields[#fields + 1], fields[#fields + 2] = event, token(action or "none")
    local line = table.concat(fields, "|")
    if route_seen[line] then return end
    route_seen[line] = true
    pcall(function() io.stdout:write(line .. "\n"); io.stdout:flush() end)
    remaining = remaining - 1
    if remaining == 0 then pcall(function() io.stdout:write(prefix .. "limit\n"); io.stdout:flush() end) end
end

function obsession_trace(ctx, desync)
    profile = token(desync.arg and desync.arg.profile)
    routing_only = desync.arg and desync.arg.routing or false
    local ok, flow = pcall(flow_identity, desync)
    current_flow = ok and flow or nil
    route("selected")
    emit("profile", profile, desync.l7payload, desync.outgoing and "out" or "in")
    return VERDICT_PASS
end

local functions = {"pass", "fake", "multisplit", "multidisorder", "multidisorder_legacy", "hostfakesplit",
    "fakedsplit", "fakeddisorder", "tls_clienthello_clone"}
for _, name in ipairs(functions) do
    local original = _G[name]
    if type(original) == "function" then
        _G[name] = function(...)
            if remaining <= 0 or os.time() > deadline then _G[name] = original; return original(...) end
            local previous = current_function
            current_function = name
            emit("function_enter", profile, name)
            local values = pack(pcall(original, ...))
            if not values[1] then
                route("action_error", name)
                emit("lua_error", profile, name, values[2])
                current_function = previous
                error(values[2], 0)
            end
            emit("function_exit", profile, name, values[2])
            route("action_ok", name)
            current_function = previous
            return unpack_values(values, 2, values.n)
        end
    end
end

-- Observe the native send primitive, not its nested Lua convenience wrappers:
-- one event is one API invocation (repeats/fragments may send multiple packets).
for _, name in ipairs({"rawsend", "rawsend_dissect"}) do
    local original = _G[name]
    if type(original) == "function" then
        _G[name] = function(...)
            if remaining <= 0 or os.time() > deadline then _G[name] = original; return original(...) end
            local values = pack(pcall(original, ...))
            if not values[1] then
                route("send_error", current_function)
                emit("send_error", profile, current_function, name, values[2])
                error(values[2], 0)
            end
            emit("send", profile, current_function, name, values[2] and "ok" or "failed")
            if current_function ~= "none" then route(values[2] and "send_ok" or "send_error", current_function) end
            return unpack_values(values, 2, values.n)
        end
    end
end
emit("ready", "v1", "metadata_only", "120s", "512events")

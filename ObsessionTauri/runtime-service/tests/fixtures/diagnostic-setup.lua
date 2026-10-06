-- No network sends: replace native primitives before loading the observer.
function diagnostic_pack(...) return {n=select("#", ...), ...} end
diagnostic_test = {lines={}, calls={}, error={fixture=true}, stdout=io.stdout}
io.stdout = {
    write=function(self, text) diagnostic_test.lines[#diagnostic_test.lines+1]=text end,
    flush=function() end,
}
function rawsend(...)
    local args = diagnostic_pack(...)
    diagnostic_test.calls[#diagnostic_test.calls+1]=args
    if args[1] == "throw" then error(diagnostic_test.error, 0) end
    return args[1] ~= "fail", nil, "tail"
end
function rawsend_dissect(...) return rawsend(...) end
function fake(ctx, desync)
    rawsend(ctx.data, nil, desync)
    return nil, 5, nil, "verdict_tail"
end
function multidisorder() error(diagnostic_test.error, 0) end
function multidisorder_legacy() error(diagnostic_test.error, 0) end
function pass() return nil end

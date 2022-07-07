function string:split(pat) -- https://stackoverflow.com/a/1647577
    pat = pat or '%s+'
    local st, g = 1, self:gmatch("()(" .. pat .. ")")
    local function getter(segs, seps, sep, cap1, ...)
        st = sep and seps + #sep
        return self:sub(segs, (seps or 0) - 1), cap1 or sep, ...
    end
    return function()
        if st then
            return getter(st, g())
        end
    end
end

function find_sinks()
    return execute('pactl', { 'list', 'sink-inputs' }):next(function(output)
        print('pactl list sink-inputs', output.code)
        local s = output.stdout
        local sinks = {}
        local sink = nil
        for line in s:split('\n') do
            local s, e = line:find("Sink Input #")
            if s and e then
                sink = tonumber(line:sub(e + 1, line:len()))
            end

            s, e = line:find("application.name = ")
            if s and e then
                s, e = line:find('%b""')
                if s and e then
                    sinks[string.lower(line:sub(s + 1, e - 1))] = sink
                end
            end
        end
        return sinks
    end)
end

function change_sink_volume(name, change, sinks)
    name = string.lower(name)
    return find_sinks():next(function(sinks)
        if sinks[name] then
            local args = { 'set-sink-input-volume', sinks[name], change }
            return execute('pactl', args)
        else
            for k, v in pairs(sinks) do
                if string.find(k, name) then
                    change_sink_volume(k, change, sinks)
                    return
                end
            end
            print("available sinks:")
            for k, v in pairs(sinks) do
                print(k, v)
            end
            local msg = "sink " .. name .. " does not exist"
            return execute("notify-send", { msg })
        end
    end)
end

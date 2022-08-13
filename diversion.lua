local Promise = require 'promise'

local current_function = 0
local execute_callbacks = {}

local function execute(cmd, args)
    return Promise:new(function(resolve)
        _G.__async_execute(current_function, cmd, args)
        execute_callbacks[current_function] = function(code, stdout, stderr)
            resolve({ code = code, stdout = stdout, stderr = stderr })
        end
        current_function = current_function + 1
    end)
end
local function exec_callback(ident, code, stdout, stderr)
    if execute_callbacks[ident] then
        execute_callbacks[ident](code, stdout, stderr)
        execute_callbacks[ident] = nil
        if #execute_callbacks == 0 then
            current_function = 0
        end
    end
end
_G.__exec_callback = exec_callback

_G.__on_event = function() end
local function listen(listener)
    _G.__on_event = listener
end

local function reload()
    _G.__reload()
end

return {
    execute = execute,
    listen = listen,
    send_event = _G.__send_event,
    reload = reload,
}

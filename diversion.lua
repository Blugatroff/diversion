local Promise = require 'promise'

local current_function = 0
local process_callbacks = {}

local function spawn(cmd, args, stdout, stderr, exit)
    local ident = current_function
    print(cmd, ident)
    current_function = current_function + 1
    _G.__async_execute(ident, cmd, args)
    process_callbacks[ident] = { stdout = stdout, stderr = stderr, exit = exit }
    return function(data)
        return _G.__process_write_stdin(ident, data)
    end
end
local function execute(cmd, args)
    return Promise:new(function(resolve)
        local stdout = ""
        local stderr = ""
        local function on_stdout(data)
            stdout = stdout .. data
        end
        local function on_stderr(data)
            stderr = stderr .. data
        end
        local function on_exit(code)
            resolve({ code = code, stdout = stdout, stderr = stderr })
        end
        spawn(cmd, args, on_stdout, on_stderr, on_exit)
    end)
end
local EXEC_CALLBACK_EXIT = 0
local EXEC_CALLBACK_STDOUT = 1
local EXEC_CALLBACK_STDERR = 2
local function exec_callback(ident, type, value)
    if process_callbacks[ident] then
        local callbacks = process_callbacks[ident]
        if type == EXEC_CALLBACK_STDOUT then
            callbacks.stdout(value)
        end
        if type == EXEC_CALLBACK_STDERR then
            callbacks.stderr(value)
        end
        if type == EXEC_CALLBACK_EXIT then
            callbacks.exit(value)
            process_callbacks[ident] = nil
        end
        if #process_callbacks == 0 then
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
    spawn = spawn,
    listen = listen,
    send_event = _G.__send_event,
    reload = reload,
    exit = _G.__exit
}

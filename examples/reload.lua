-- press F12 to reload your script

dofile './codes.lua'
local Promise = require 'promise'
local diversion = require 'diversion'

local function on_event(device, ty, code, value)
    if ty == EV_KEY and code == F2 then
        if value == 1 then
            diversion.reload()
        end
        return
    end
    diversion.send_event(ty, code, value)
end

diversion.listen(on_event)

diversion.execute("whoami", {}):next(function(output)
    print("started at " .. os.date("%Y-%m-%d %H:%M:%S") .. " as user" .. output.stdout)
end)

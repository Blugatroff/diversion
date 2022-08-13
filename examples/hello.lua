-- flip A and B

dofile './codes.lua'
local Promise = require 'promise'
local diversion = require 'diversion'

local function on_event(device, ty, code, value)
    if ty == EV_KEY then
        if code == A then
            diversion.send_event(ty, B, value)
            return
        elseif code == B then
            diversion.send_event(ty, A, value)
            return
        end
    end
    diversion.send_event(ty, code, value)
end

diversion.listen(on_event)

diversion.execute("whoami", {}):next(function(output)
    print("started at " .. os.date("%Y-%m-%d %H:%M:%S") .. " as user" .. output.stdout)
end)

-- hold L_CTRL to reverse all mouse movement

dofile './codes.lua'
local Promise = require 'promise'
local diversion = require 'diversion'

local ctrl_pressed = false

local function on_event(device, ty, code, value)
    if ty == EV_KEY and code == L_CTRL then
        ctrl_pressed = value ~= 0
    end
    if ty == EV_REL and ctrl_pressed then
        -- reverse the mouse movement
        diversion.send_event(ty, code, -value)
        return
    end
    diversion.send_event(ty, code, value)
end

diversion.listen(on_event)

diversion.execute("whoami", {}):next(function(output)
    print("started at " .. os.date("%Y-%m-%d %H:%M:%S") .. " as user" .. output.stdout)
end)

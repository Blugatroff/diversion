print("hold F2 to print every event")

local f2_pressed = false

local function on_event(device, ty, code, value)
    if ty == EV_KEY and code == F2 then
        f2_pressed = value ~= 0
    end
    if f2_pressed then
        print("device: " .. device .. " ty: " .. ty .. " code: " .. code .. " value: " .. value)
    end
    diversion.send_event(ty, code, value)
end

diversion.listen(on_event)

diversion.execute("whoami", {}):next(function(output)
    print("started at " .. os.date("%Y-%m-%d %H:%M:%S") .. " as user" .. output.stdout)
end)

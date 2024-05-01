print("press F2 to reload your script")

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

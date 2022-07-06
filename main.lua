dofile './util.lua'
local Promise = dofile './promise.lua'

EV_SYN = 0
EV_KEY = 1
EV_REL = 2
EV_ABS = 3
EV_MSC = 4

X = 0
Y = 1

ESCAPE = 1
CAPS_LOCK = 58
R_FN = 126
MENU = 127
I = 23
P = 25
L_CTRL = 29
D = 32
F = 33
H = 35
J = 36
K = 37
L = 38
SEMICOLON = 39
N = 49
M = 50
L_ALT = 56
SPACE = 57
L_PIPE = 86
R_CTRL = 97
INSERT = 110
VOL_DOWN = 114
VOL_UP = 115
PAUSE_BREAK = 119
L_BUTTON = 272
R_BUTTON = 273
M_BUTTON = 274

KEYS_DOWN = {}

function create_mouse_callback(device, key, axis, direction)
    return function(value)
        if KEYS_DOWN[device][L_PIPE] then
            if value == 1 or value == 2 then
                if KEYS_DOWN[device][D] and KEYS_DOWN[device][F] then
                    send_event(EV_REL, axis, 10 * direction)
                elseif KEYS_DOWN[device][D] then
                    send_event(EV_REL, axis, 50 * direction)
                elseif KEYS_DOWN[device][F] then
                    send_event(EV_REL, axis, 4 * direction)
                else
                    send_event(EV_REL, axis, 200 * direction)
                end
            end
        else
            send_event(EV_KEY, key, value)
        end
    end
end

DISABLED = function() end

CORSAIR = 0
PUGIO = 1

OVERRIDES = {
    [CORSAIR] = {
        [EV_KEY] = {
            [R_FN] = DISABLED,
            [MENU] = function(value)
                if value == 1 then
                    execute("sleep 100")
                end
            end,
            [L_PIPE] = DISABLED,
            [D] = function(value)
                if not KEYS_DOWN[CORSAIR][L_PIPE] then
                    send_event(EV_KEY, D, value)
                end
            end,
            [F] = function(value)
                if not KEYS_DOWN[CORSAIR][L_PIPE] then
                    send_event(EV_KEY, F, value)
                end
            end,
            [SPACE] = function(value)
                if KEYS_DOWN[CORSAIR][L_PIPE] then
                    send_event(EV_KEY, L_BUTTON, value)
                else
                    send_event(EV_KEY, SPACE, value)
                end
            end,
            [N] = function(value)
                if KEYS_DOWN[CORSAIR][L_PIPE] then
                    send_event(EV_KEY, R_BUTTON, value)
                else
                    send_event(EV_KEY, N, value)
                end
            end,
            [M] = function(value)
                if KEYS_DOWN[CORSAIR][L_PIPE] then
                    send_event(EV_KEY, M_BUTTON, value)
                else
                    send_event(EV_KEY, M, value)
                end
            end,
            [P] = function(value)
                if KEYS_DOWN[CORSAIR][L_PIPE] then
                    if value == 1 or value == 2 then
                        send_event(EV_REL, 11, 100)
                    end
                else
                    send_event(EV_KEY, P, value)
                end
            end,
            [SEMICOLON] = function(value)
                if KEYS_DOWN[CORSAIR][L_PIPE] then
                    if value == 1 or value == 2 then
                        send_event(EV_REL, 11, -100)
                    end
                else
                    send_event(EV_KEY, SEMICOLON, value)
                end
            end,
            [L_ALT] = function(value)
                send_event(EV_KEY, L_ALT, value)
            end,
            [ESCAPE] = function(value)
                send_event(EV_KEY, CAPS_LOCK, value)
            end,
            [CAPS_LOCK] = function(value)
                send_event(EV_KEY, ESCAPE, value)
            end,
            [H] = create_mouse_callback(CORSAIR, H, X, -1),
            [J] = create_mouse_callback(CORSAIR, J, Y, 1),
            [L] = create_mouse_callback(CORSAIR, L, X, 1),
            [K] = create_mouse_callback(CORSAIR, K, Y, -1),
            [VOL_DOWN] = function(value)
                if KEYS_DOWN[CORSAIR][L_CTRL] then
                    change_sink_volume("Spotify", '-5%')
                else
                    send_event(EV_KEY, VOL_DOWN, value)
                end
            end,
            [VOL_UP] = function(value)
                if KEYS_DOWN[CORSAIR][L_CTRL] then
                    change_sink_volume("Spotify", '+5%')
                else
                    send_event(EV_KEY, VOL_UP, value)
                end
            end,
            [PAUSE_BREAK] = function(value)
                if KEYS_DOWN[CORSAIR][L_CTRL] then
                    reload()
                else
                    send_event(EV_KEY, PAUSE_BREAK, value)
                end
            end
        }
    },
    [PUGIO] = {
        [EV_REL] = {
            [X] = function(value)
                send_event(EV_REL, X, value)
            end,
            [Y] = function(value)
                send_event(EV_REL, Y, value)
            end
        },
        [EV_KEY] = {
            [L_BUTTON] = function(value)
                send_event(EV_KEY, L_BUTTON, value)
            end
        }
    }
}

function on_event(device, ty, code, value)
    if KEYS_DOWN[device] == nil then
        KEYS_DOWN[device] = {}
    end
    local keys_down = KEYS_DOWN[device]
    if ty == EV_KEY then
        keys_down[code] = value ~= 0
    end
    if keys_down[INSERT] then
        print(ty, code, value)
        return
    end
    local device_override = OVERRIDES[device]
    if device_override ~= nil then
        local ty_override = device_override[ty]
        if ty_override ~= nil then
            local override = ty_override[code]
            if override ~= nil then
                override(value)
                return
            end
        end
    end
    send_event(ty, code, value)
end

current_function = 0
execute_callbacks = {}
function execute(cmd)
    return Promise:new(function(resolve) 
        native_execute(current_function, cmd)
        execute_callbacks[current_function] = resolve
        current_function = current_function + 1
    end)
end

function exec_callback(ident, output)
    if execute_callbacks[ident] then
        execute_callbacks[ident](output)
        execute_callbacks[ident] = nil
    end
end

execute({ "date" }):next(function(date)
    print(date)
end)

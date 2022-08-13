local Promise = {}
local PromiseMt = { __index = Promise }

function Promise:new(f)
    local promise = {
        done = false,
        listeners = {}
    }
    local function resolve(value)
        if getmetatable(value) == PromiseMt then
            value:next(resolve)
        else
            promise.done = true
            promise.value = value;
            for k, f in pairs(promise.listeners) do
                if type(f) == "function" then
                    f(value)
                end
            end
            promise.listeners = {}
        end
    end
    f(resolve)
    return setmetatable(promise, PromiseMt)
end

function Promise:done(v)
    return Promise:new(function(resolve)
        resolve(v)
    end)
end

function Promise:next(f)
    if self.done then
        return Promise:new(function(resolve) 
            resolve(f(self.value)) 
        end)
    else
        return Promise:new(function(resolve)
            table.insert(self.listeners, function(value)
                resolve(f(value))
            end) 
        end)
    end
end

return Promise

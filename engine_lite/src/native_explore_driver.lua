local capture = wesnoth.require("~add-ons/RustnothAudit/capture.lua")
local actions = wesnoth.require("~add-ons/RustnothAudit/actions.lua")
local choose = wesnoth.require("~add-ons/RustnothAudit/choice.lua")
local step = 0

local function marker(message)
    wesnoth.log("info", "RUSTNOTH_AUDIT_" .. message, false)
end

local function run(time_over)
    local turn_action_count = 0
    while true do
        -- End-turn commands reach this checkpoint on the next refresh or time over.
        local terminal = capture.snapshot(step, time_over)
        if terminal or step == limit then
            wesnoth.wml_actions.endlevel { test_result = "pass" }
            return
        end
        step = step + 1
        local command = choose(seed, step, turn_action_count)
        marker("COMMAND " .. step .. "\t" .. table.concat(command, "\t"))
        local kind = command[1]
        if kind == "end_turn" then
            actions.end_turn()
            return
        elseif kind == "attack" then
            marker("ATTACK_BEGIN " .. step)
            actions.attack(table.unpack(command, 2))
            marker("ATTACK_END " .. step)
        elseif kind == "recruit" then
            marker("RECRUIT_BEGIN " .. step)
            local trait_draws = actions.recruit(table.unpack(command, 2))
            marker("RECRUIT_END " .. step .. " " .. trait_draws)
        elseif kind == "move" then
            actions.move(table.unpack(command, 2))
        else
            error("unknown exploration command: " .. tostring(kind))
        end
        turn_action_count = turn_action_count + 1
    end
end

return { run = run }

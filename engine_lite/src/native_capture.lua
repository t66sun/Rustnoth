local capture = {}

local escapes = {
    ["\\"] = "\\\\",
    ["\t"] = "\\t",
    ["\r"] = "\\r",
    ["\n"] = "\\n",
}

local function escape_id(text)
    return (text:gsub("[\\\t\r\n]", escapes))
end

local function row(...)
    std_print(table.concat({...}, "\t"))
end

local function hex_order(a, b)
    return a.x < b.x or (a.x == b.x and a.y < b.y)
end

local function owned_villages()
    local owned = {}
    for _, location in ipairs(wesnoth.map.find { gives_income = true }) do
        local x, y = location[1], location[2]
        local side = wesnoth.map.get_owner(x, y)
        if side and side > 0 then
            owned[#owned + 1] = { x = x, y = y, side = side }
        end
    end
    table.sort(owned, hex_order)
    return owned
end

local function surviving_sides(units)
    local leaders, present = {}, {}
    for _, unit in ipairs(units) do
        present[unit.side] = true
        if unit.canrecruit then
            leaders[unit.side] = true
        end
    end
    local sides = {}
    for side = 1, #wesnoth.sides do
        local condition = wesnoth.sides[side].defeat_condition
        if condition == "never" or (condition == "no_units_left" and present[side])
            or (condition == "no_leader_left" and leaders[side]) then
            sides[#sides + 1] = side
        end
    end
    return sides
end

local function enemies_remain(sides)
    for i, side in ipairs(sides) do
        for j = i + 1, #sides do
            if wesnoth.sides.is_enemy(side, sides[j]) then
                return true
            end
        end
    end
    return false
end

local function capture_views()
    local map = wesnoth.current.map
    for side = 1, #wesnoth.sides do
        local team = wesnoth.sides[side]
        if team.fog or team.shroud then
            for y = 1, map.playable_height do
                local tiles = {}
                for x = 1, map.playable_width do
                    if wesnoth.sides.is_shrouded(side, x, y) then
                        tiles[x] = "S"
                    elseif wesnoth.sides.is_fogged(side, x, y) then
                        tiles[x] = "F"
                    else
                        tiles[x] = "V"
                    end
                end
                row("view", side, y, table.concat(tiles))
            end
            local visible = wesnoth.units.find_on_map {
                wml.tag.filter_vision { side = side, visible = true },
                wml.tag["or"] { side = side },
            }
            table.sort(visible, hex_order)
            local seen = { "seen", side }
            for _, unit in ipairs(visible) do
                seen[#seen + 1] = unit.x .. "," .. unit.y
            end
            std_print(table.concat(seen, "\t"))
        end
    end
end

-- Call after an action or at side-turn refresh, inside the referee only.
function capture.snapshot(step, time_over)
    local units = wesnoth.units.find_on_map {}
    table.sort(units, hex_order)
    local sides = surviving_sides(units)
    local manual = false
    for _, side in ipairs(sides) do
        if capture.manual_victory_sides and capture.manual_victory_sides[side] then
            manual = true
            break
        end
    end
    local terminal = time_over or (not manual and not enemies_remain(sides))
    if time_over or #sides == 0 then
        row("step", step, "draw")
    elseif terminal then
        row("step", step, "won", table.unpack(sides))
    else
        row("step", step, "playing", wesnoth.current.turn, wesnoth.current.side)
    end

    for side = 1, #wesnoth.sides do
        row("gold", side, wesnoth.sides[side].gold)
    end
    local slots = {}
    for _, unit in ipairs(wesnoth.units.find_on_recall {}) do
        local side = unit.side
        slots[side] = (slots[side] or 0) + 1
        local traits = {}
        for i, id in ipairs(unit.traits) do traits[i] = escape_id(id) end
        local cost = unit.recall_cost
        if cost < 0 then cost = wesnoth.sides[side].recall_cost end
        row("reserve", side, slots[side], escape_id(unit.type),
            unit.hitpoints, unit.experience, cost, table.unpack(traits))
    end
    for _, unit in ipairs(units) do
        local unit_type = escape_id(unit.type)
        local traits = {}
        for index, id in ipairs(unit.traits) do
            traits[index] = escape_id(id)
        end
        row("unit", unit.x, unit.y, unit.side, unit_type, unit.hitpoints,
            unit.moves, unit.attacks_left, unit.canrecruit and 1 or 0, unit.experience,
            table.unpack(traits))
        local statuses = {}
        if unit.status.slowed then statuses[#statuses + 1] = "slowed" end
        if unit.status.poisoned then statuses[#statuses + 1] = "poisoned" end
        if unit.status.petrified then statuses[#statuses + 1] = "petrified" end
        if #statuses > 0 then
            row("status", unit.x, unit.y, table.unpack(statuses))
        end
    end
    for _, village in ipairs(owned_villages()) do
        row("village", village.x, village.y, village.side)
    end
    capture_views()
    row("end", step)
    return terminal
end

return capture

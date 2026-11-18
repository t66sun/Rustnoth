local capture = {}

local escapes = {
    ["\\"] = "\\\\",
    ["\t"] = "\\t",
    ["\r"] = "\\r",
    ["\n"] = "\\n",
}

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

-- Call after an action or at side-turn refresh, inside the referee only.
function capture.snapshot(step, time_over)
    local units = wesnoth.units.find_on_map {}
    table.sort(units, hex_order)

    local leaders = {}
    local leader_sides = 0
    local winning_side
    for _, unit in ipairs(units) do
        if unit.canrecruit and not leaders[unit.side] then
            leaders[unit.side] = true
            leader_sides = leader_sides + 1
            winning_side = unit.side
        end
    end

    local terminal = time_over or leader_sides <= 1
    if time_over or leader_sides == 0 then
        row("step", step, "draw")
    elseif leader_sides == 1 then
        row("step", step, "won", winning_side)
    else
        row("step", step, "playing", wesnoth.current.turn, wesnoth.current.side)
    end

    for side = 1, #wesnoth.sides do
        row("gold", side, wesnoth.sides[side].gold)
    end
    for _, unit in ipairs(units) do
        local unit_type = unit.type:gsub("[\\\t\r\n]", escapes)
        local traits = {}
        for index, id in ipairs(unit.traits) do
            traits[index] = id:gsub("[\\\t\r\n]", escapes)
        end
        row("unit", unit.x, unit.y, unit.side, unit_type, unit.hitpoints,
            unit.moves, unit.attacks_left, unit.canrecruit and 1 or 0, unit.experience,
            table.unpack(traits))
        local statuses = {}
        if unit.status.slowed then statuses[#statuses + 1] = "slowed" end
        if unit.status.poisoned then statuses[#statuses + 1] = "poisoned" end
        if #statuses > 0 then
            row("status", unit.x, unit.y, table.unpack(statuses))
        end
    end
    for _, village in ipairs(owned_villages()) do
        row("village", village.x, village.y, village.side)
    end
    row("end", step)
    return terminal
end

return capture

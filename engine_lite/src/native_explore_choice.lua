local function position_key(x, y)
    return x .. "," .. y
end

local function hex_order(a, b)
    return a.x < b.x or (a.x == b.x and a.y < b.y)
end

local function pick(options, seed, step)
    if #options > 0 then
        local index = ((seed % 2147483647) * 48271 + step * 69621) % 2147483647
        return options[1 + index % #options]
    end
end

-- Choose one ordinary player command for the differential runner, without consuming game RNG.
return function(seed, step, turn_action_count)
    if turn_action_count >= 8 then return { "end_turn" } end
    local side = wesnoth.current.side
    local team = wesnoth.sides[side]
    local map = wesnoth.current.map
    local own = wesnoth.units.find_on_map { side = side }
    table.sort(own, hex_order)
    local occupied, enemies = {}, {}
    local function mark(unit)
        occupied[position_key(unit.x, unit.y)] = unit
    end
    for _, unit in ipairs(own) do mark(unit) end
    for _, unit in ipairs(wesnoth.units.find_on_map {
        wml.tag.filter_vision { side = side, visible = true },
    }) do
        mark(unit)
        if wesnoth.sides.is_enemy(side, unit.side) and not unit.status.petrified then
            enemies[#enemies + 1] = unit
        end
    end

    -- The native audit recruit command uses the first leader returned by the original engine.
    local recruiter = wesnoth.units.find_on_map { side = side, canrecruit = true }[1]
    local recruits = {}
    for _, id in ipairs(team.recruit) do
        if wesnoth.unit_types[id].cost <= team.gold then recruits[#recruits + 1] = id end
    end
    table.sort(recruits)

    local attacks, placements, moves = {}, {}, {}
    local best_distance = math.huge
    for _, unit in ipairs(own) do
        if not unit.status.petrified then
            local on_keep = #recruits > 0 and recruiter and unit.x == recruiter.x and unit.y == recruiter.y
                and not wesnoth.sides.is_shrouded(side, unit.x, unit.y)
                and wesnoth.terrain_types[map[{ unit.x, unit.y }]].keep
            for x, y in map:iter_adjacent(unit) do
                local target = occupied[position_key(x, y)]
                if target then
                    if unit.attacks_left > 0 and wesnoth.sides.is_enemy(side, target.side)
                        and not target.status.petrified then
                        for i, weapon in ipairs(unit.attacks) do
                            local cfg = weapon.__cfg
                            if (tonumber(cfg.min_range) or 1) <= 1 and (tonumber(cfg.max_range) or 1) >= 1 then
                                attacks[#attacks + 1] = { "attack", unit.x, unit.y, x, y, i - 1 }
                            end
                        end
                    end
                elseif not wesnoth.sides.is_shrouded(side, x, y) then
                    if on_keep and not wesnoth.sides.is_fogged(side, x, y)
                        and wesnoth.terrain_types[map[{ x, y }]].castle then
                        for _, id in ipairs(recruits) do
                            placements[#placements + 1] = { "recruit", x, y, id }
                        end
                    end
                    if unit.moves > 0 then
                        local route, cost = wesnoth.paths.find_path(unit, x, y, {
                            viewing_side = side, ignore_teleport = true, max_cost = unit.moves + 1,
                        })
                        if #route == 2 and cost <= unit.moves then
                            local distance = math.huge
                            for _, enemy in ipairs(enemies) do
                                distance = math.min(distance, wesnoth.map.distance_between(x, y, enemy.x, enemy.y))
                            end
                            if #enemies == 0 then
                                distance = wesnoth.map.distance_between(x, y,
                                    math.ceil(map.playable_width / 2), math.ceil(map.playable_height / 2))
                            end
                            if distance < best_distance then moves, best_distance = {}, distance end
                            if distance == best_distance then moves[#moves + 1] = { "move", unit.x, unit.y, x, y } end
                        end
                    end
                end
            end
        end
    end
    return pick(attacks, seed, step) or pick(placements, seed, step) or pick(moves, seed, step) or { "end_turn" }
end

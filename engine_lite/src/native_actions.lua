local actions = {}
local tag = wml.tag

function actions.move(from_x, from_y, to_x, to_y)
    local unit = assert(wesnoth.units.get(from_x, from_y), "move source has no unit")
    local path = wesnoth.paths.find_path(unit, to_x, to_y)
    assert(#path > 1, "native path has no move to the requested destination")

    local xs, ys = {}, {}
    for i, hex in ipairs(path) do
        xs[i], ys[i] = hex.x, hex.y
    end
    wesnoth.wml_actions.do_command {
        tag.move { x = table.concat(xs, ","), y = table.concat(ys, ",") },
    }
end

function actions.attack(from_x, from_y, to_x, to_y, weapon)
    local attacker = assert(wesnoth.units.get(from_x, from_y))
    local target = assert(wesnoth.units.get(to_x, to_y))
    local _, _, _, response = wesnoth.simulate_combat(attacker, weapon + 1, target)
    wesnoth.wml_actions.do_command {
        tag.attack {
            weapon = weapon,
            defender_weapon = response.weapon and response.number - 1 or -1,
            tag.source { x = from_x, y = from_y },
            tag.destination { x = to_x, y = to_y },
        },
    }
end

function actions.recruit(x, y, type_id)
    local leaders = wesnoth.units.find_on_map { side = wesnoth.current.side, canrecruit = true }
    local leader = assert(leaders[1], "recruiting side has no leader")
    wesnoth.wml_actions.do_command {
        tag.recruit {
            type = type_id,
            x = x,
            y = y,
            tag.from { x = leader.x, y = leader.y },
        },
    }
    local unit = assert(wesnoth.units.get(x, y), "native recruit did not place a unit")
    local modifications = wml.get_child(unit.__cfg, "modifications")
    local trait_draws = 0
    if modifications then
        for trait in wml.child_range(modifications, "trait") do
            if trait.availability ~= "musthave" then
                trait_draws = trait_draws + 1
            end
        end
    end
    return trait_draws
end

function actions.recall(x, y, unit_id)
    local side = wesnoth.current.side
    local veteran = assert(wesnoth.units.find_on_recall { side = side, id = unit_id }[1], "missing reserve")
    local map = wesnoth.current.map
    local limit = map.playable_width + map.playable_height
    local function castle_cost(cx, cy)
        if not wesnoth.sides.is_shrouded(side, cx, cy) and wesnoth.terrain_types[map[{cx, cy}]].castle then
            return 1
        end
        return limit + 1
    end
    assert(castle_cost(x, y) == 1 and not wesnoth.units.get(x, y), "recall needs a vacant known castle")
    local leader
    for _, candidate in ipairs(wesnoth.units.find_on_map { side = side, canrecruit = true }) do
        if wesnoth.terrain_types[map[{candidate.x, candidate.y}]].keep
            and wesnoth.units.matches(veteran, candidate.recall_filter) then
            local path = wesnoth.paths.find_path(candidate, x, y, {
                calculate = castle_cost, ignore_teleport = true, max_cost = limit,
            })
            if #path > 1 then
                leader = candidate
                break
            end
        end
    end
    assert(leader, "no eligible recall leader on a connected keep")
    wesnoth.wml_actions.do_command {
        tag.recall { value = unit_id, x = x, y = y, tag.from { x = leader.x, y = leader.y } },
    }
    local unit = wesnoth.units.get(x, y)
    assert(unit and unit.id == unit_id, "native recall did not place the requested unit")
end

function actions.disband(unit_id)
    local veteran = { id = unit_id, side = wesnoth.current.side }
    assert(#wesnoth.units.find_on_recall(veteran) == 1, "native disband needs an owned reserve")
    wesnoth.wml_actions.do_command { tag.disband { value = unit_id } }
    assert(#wesnoth.units.find_on_recall(veteran) == 0, "native disband left the veteran on recall")
end

function actions.end_turn()
    wesnoth.wml_actions.end_turn {}
end

return actions

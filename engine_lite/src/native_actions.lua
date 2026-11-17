local actions = {}
local tag = wml.tag

function actions.move(from_x, from_y, to_x, to_y)
    local unit = assert(wesnoth.units.get(from_x, from_y), "move source has no unit")
    local path = wesnoth.paths.find_path(unit, to_x, to_y, { ignore_teleport = true })
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
    wesnoth.wml_actions.do_command {
        tag.attack {
            weapon = weapon,
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
end

function actions.end_turn()
    wesnoth.wml_actions.end_turn {}
end

return actions

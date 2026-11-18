-- Run with Lua 5.4 from the repository root.
local leader = { x = 2, y = 1, side = 1, canrecruit = true, moves = 5, attacks_left = 1,
    attacks = { { __cfg = { min_range = 2 } }, { __cfg = {} } }, status = {} }
local enemy = { x = 3, y = 1, side = 2, status = {} }
local visible_enemy, hidden_enemy, castle, shrouded = false, true, false, true
local map = { playable_width = 3, playable_height = 1 }
function map:iter_adjacent()
    local xs, i = { 1, 3 }, 0
    return function()
        i = i + 1
        if xs[i] then return xs[i], 1 end
    end
end
setmetatable(map, { __index = function(_, hex)
    assert(type(hex) == "table" and not (shrouded and hex[1] == 3), "read hidden terrain")
    if hex[1] == 2 then return "Kh" end
    return castle and "Ch" or "Gg"
end })

wml = { tag = { filter_vision = function(cfg) return { "filter_vision", cfg } end } }
wesnoth = {
    current = { side = 1, map = map },
    map = { distance_between = function(x1, _, x2) return math.abs(x1 - x2) end },
    unit_types = { Scout = { cost = 15 } },
    terrain_types = { Kh = { keep = true }, Ch = { castle = true }, Gg = {} },
    units = { find_on_map = function(filter)
        if filter.side == 1 then return { leader } end
        assert(filter[1] and filter[1][1] == "filter_vision" and filter[1][2].side == 1
            and filter[1][2].visible, "unmasked foreign-unit query")
        return hidden_enemy and visible_enemy and { leader, enemy } or { leader }
    end },
    sides = {
        [1] = { recruit = { "Scout" }, gold = 0 },
        is_shrouded = function(side, x) assert(side == 1) return shrouded and x == 3 end,
        is_fogged = function(side, x) assert(side == 1) return x == 3 and not visible_enemy end,
        is_enemy = function(a, b) return a ~= b end,
    },
    paths = { find_path = function(unit, x, y, options)
        assert(unit == leader and options.viewing_side == 1 and options.ignore_teleport)
        assert(not (shrouded and x == 3), "path query into shroud")
        return { { unit.x, unit.y }, { x, y } }, 1
    end },
}
local choose = dofile(arg[1] or "engine_lite/src/native_explore_choice.lua")
local function action(seed)
    return table.concat(choose(seed, 1, 0), "\t")
end
assert(action(1) == "move\t2\t1\t1\t1")
hidden_enemy = false
assert(action(1) == "move\t2\t1\t1\t1")
shrouded = false
local fogged = action(0)
hidden_enemy = true
assert(action(0) == fogged, "unseen occupant changed movement")
assert(action(0) ~= action(1), "seed must vary equivalent moves")

castle = true
wesnoth.sides[1].gold = 15
assert(action(1) == "recruit\t1\t1\tScout")
visible_enemy = true
assert(action(1) == "attack\t2\t1\t3\t1\t1")
leader.moves, leader.attacks_left, wesnoth.sides[1].gold = 0, 0, 0
assert(action(1) == "end_turn")
leader.attacks_left = 1
assert(choose(1, 9, 8)[1] == "end_turn")

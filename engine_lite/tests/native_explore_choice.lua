-- Run with Lua 5.4 from the repository root.
local leader = { x = 2, y = 1, side = 1, canrecruit = true, moves = 5, attacks_left = 1,
    attacks = { { __cfg = { min_range = 2 } }, { __cfg = {} } }, status = {}, recall_filter = {} }
local enemy = { x = 3, y = 1, side = 2, status = {} }
local visible_enemy, hidden_enemy, castle, shrouded = false, true, false, true
local roster = {}
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
    end,
    find_on_recall = function(filter)
        assert(filter.side == 1, "asked for another side's recall roster")
        return roster
    end,
    matches = function(unit, filter) return not filter.id or unit.id == filter.id end },
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

visible_enemy = false
wesnoth.sides[1].recruit = {}
wesnoth.sides[1].recall_cost = 18
wesnoth.sides[1].gold = 18
roster = { { id = "west_guard", type = "Swordsman", recall_cost = -1 },
    { id = "east_guard", type = "Swordsman", recall_cost = 13 } }
assert(action(0) == "recall\t1\t1\twest_guard")
assert(action(1) == "recall\t1\t1\teast_guard")
leader.recall_filter = { id = "east_guard" }
assert(action(0) == "recall\t1\t1\teast_guard")
leader.recall_filter = {}
wesnoth.sides[1].gold = 13
assert(action(0) == "recall\t1\t1\teast_guard")
wesnoth.sides[1].gold = 12
assert(action(0) == "end_turn")

wesnoth.sides[1].gold = 13
local markers, executed = {}, 0
wesnoth.log = function(_, msg) markers[#markers + 1] = msg end
wesnoth.wml_actions = { endlevel = function(cfg) assert(cfg.test_result == "pass") end }
wesnoth.require = function(path)
    if path:match("capture%.lua$") then return { snapshot = function() return false end } end
    if path:match("choice%.lua$") then return choose end
    assert(path:match("actions%.lua$"))
    return { recall = function(x, y, id)
        assert(x == 1 and y == 1 and id == "east_guard")
        executed = executed + 1
    end }
end
seed, limit = 0, 1
dofile("engine_lite/src/native_explore_driver.lua").run(false)
assert(executed == 1 and #markers == 1)
assert(markers[1] == "RUSTNOTH_AUDIT_COMMAND 1\trecall\t1\t1\teast_guard")

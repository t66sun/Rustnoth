-- Run with Lua 5.4 from the repository root.
local troops = {}
local rows = {}
function std_print(line) rows[#rows + 1] = line end
wesnoth = {
    current = { turn = 1, side = 1, map = {} },
    sides = { { gold = 0, defeat_condition = "no_leader_left" }, { gold = 0 }, is_enemy = function(a, b) return a ~= b end },
    units = { find_on_map = function() return troops end, find_on_recall = function() return {} end },
    map = { find = function() return {} end },
}
local function unit(side, leader)
    return { side = side, x = side, y = 1, canrecruit = leader, type = "Spearman", hitpoints = 30,
        moves = 5, attacks_left = 1, experience = 0, traits = {}, status = {} }
end
local capture = dofile("engine_lite/src/native_capture.lua")
local function checkpoint(condition, second, expected)
    rows = {}
    troops = { unit(1, true), second }
    wesnoth.sides[2].defeat_condition = condition
    capture.snapshot(0, false)
    assert(rows[1] == "step\t0\t" .. expected, rows[1])
end
checkpoint("no_leader_left", unit(2, false), "won\t1")
checkpoint("no_units_left", unit(2, false), "playing\t1\t1")
checkpoint("no_units_left", nil, "won\t1")
checkpoint("never", nil, "playing\t1\t1")
checkpoint("always", unit(2, true), "won\t1")

-- Run with Lua 5.4 from the repository root.
local attacker, defender = {}, {}
local expected, recorded
wml = { tag = { attack = function(cfg) return cfg end, source = function(cfg) return cfg end,
    destination = function(cfg) return cfg end } }
wesnoth = {
    units = { get = function(x) return x == 1 and attacker or defender end },
    simulate_combat = function(a, weapon, d)
        assert(a == attacker and weapon == 2 and d == defender)
        return {}, {}, {}, expected
    end,
    wml_actions = { do_command = function(cfg) recorded = cfg[1] end },
}
local actions = dofile("engine_lite/src/native_actions.lua")
for i, choice in ipairs({ { weapon = {}, number = 2 }, { weapon = {}, number = 1 }, { number = 0 } }) do
    expected = choice
    actions.attack(1, 1, 2, 1, 1)
    assert(recorded.weapon == 1 and recorded.defender_weapon == ({1, 0, -1})[i])
end

local distance = wesnoth.map.distance_between
local function nearer(a, b) return a.x < b.x or (a.x == b.x and a.y < b.y) end
return function()
    local side = wesnoth.current.side
    local own = wesnoth.units.find_on_map {side=side}
    table.sort(own, nearer)
    local seen = wesnoth.units.find_on_map { wml.tag.filter_vision {side=side, visible=true} }
    local enemies, occupied = {}, {}
    for _,u in ipairs(seen) do
        occupied[u.x..","..u.y] = u
        if wesnoth.sides.is_enemy(side,u.side) then enemies[#enemies+1]=u end
    end
    table.sort(enemies,nearer)
    local liberated = false
    for _,u in ipairs(own) do if u.id=="Minister Hylas" then liberated=true end end
    local best, high = {"end_turn"}, 0
    local function offer(score,action) if score > high then high,best = score,action end end
    for _,u in ipairs(own) do
        for _,v in ipairs(enemies) do
            if u.attacks_left>0 and distance(u.x,u.y,v.x,v.y)==1 then
                for i,_ in ipairs(u.attacks) do
                    local a,d=wesnoth.simulate_combat(u,i,v)
                    local reward=(v.hitpoints-d.average_hp)*3-(u.hitpoints-a.average_hp)*2
                    if not (u.id=="Deoran" and a.average_hp<25 and not v.canrecruit) then
                        offer(1000+reward+(v.canrecruit and 100 or 0)+100*(d.hp_chance[0] or 0),{"attack",u.x,u.y,v.x,v.y,i-1})
                    end
                end
            end
        end
        if u.moves>0 then
            local locations=wesnoth.paths.find_reach(u)
            table.sort(locations,nearer)
            for _,p in ipairs(locations) do
                if not occupied[p.x..","..p.y] then
                    if side==1 and not liberated then
                        local d=distance(p.x,p.y,22,13)
                        if d<distance(u.x,u.y,22,13) and (d>0 or u.id=="Deoran") then
                            offer(330-6*d+(u.id=="Deoran" and 10 or 0)+(d==0 and 2000 or 0),{"move",u.x,u.y,p.x,p.y})
                        end
                    else
                        for _,v in ipairs(enemies) do
                            local d=distance(p.x,p.y,v.x,v.y)
                            if d<distance(u.x,u.y,v.x,v.y) and not (side==1 and u.canrecruit and u.hitpoints<33) then
                                local value=100-5*d+(v.canrecruit and 20 or 0)+(side==1 and not u.canrecruit and 20 or 0)
                                if d==1 and u.attacks_left>0 then value=value+24 end
                                offer(value,{"move",u.x,u.y,p.x,p.y})
                            end
                        end
                    end
                end
            end
        end
        if u.canrecruit and wesnoth.terrain_types[wesnoth.current.map[{u.x,u.y}]].keep and #own<(side==1 and 8 or 5) then
            local kinds=wesnoth.sides[side].recruit
            table.sort(kinds)
            for _,kind in ipairs(kinds) do
                if wesnoth.unit_types[kind].cost <= wesnoth.sides[side].gold then
                    local sc=360+(kind=="Spearman" and 2 or 0)
                    for x,y in wesnoth.current.map:iter_adjacent(u.x,u.y) do
                        if wesnoth.terrain_types[wesnoth.current.map[{x,y}]].castle and not occupied[x..","..y] then
                            offer(sc,{"recruit",x,y,kind})
                        end
                    end
                end
            end
        end
    end
    return best
end

local route = {
 {"move",14,5,12,8},{"move",15,5,10,7},{"move",10,7,9,9},{"move",12,8,11,9},{"end_turn"},
 {"recruit",14,18,"Ruffian"},{"recruit",14,19,"Ruffian"},{"move",15,19,10,18},{"end_turn"},
 {"move",11,9,9,10},{"move",9,9,11,12},{"move",12,12,10,16},{"move",10,9,10,14},{"move",9,10,10,12},
 {"end_turn"},{"move",10,18,9,17},{"attack",9,17,10,16,0},{"move",14,18,10,18},{"move",14,19,9,19},{"end_turn"},
 {"attack",10,16,9,17,1}
}
local distance = wesnoth.map.distance_between
local function available(command, occupied)
 local kind=command[1]
 if kind=="end_turn" then return true end
 local unit=occupied[command[2]..","..command[3]]
 if kind=="recruit" then return not unit and wesnoth.sides[wesnoth.current.side].gold>=wesnoth.unit_types[command[4]].cost end
 if not unit or unit.side~=wesnoth.current.side then return false end
 local target=occupied[command[4]..","..command[5]]
 if kind=="attack" then return unit.attacks_left>0 and target and wesnoth.sides.is_enemy(unit.side,target.side) end
 if target then return false end
 for _,p in ipairs(wesnoth.paths.find_reach(unit)) do
  if p.x==command[4] and p.y==command[5] then return true end
 end
 return false
end
return function(_,step)
 local side=wesnoth.current.side
 local enemies=wesnoth.units.find_on_map{wml.tag.filter_vision{side=side,visible=true}}
 local occupied={}
 for _,unit in ipairs(enemies) do occupied[unit.x..","..unit.y]=unit end
 if route[step] and available(route[step], occupied) then return route[step] end
 local own=wesnoth.units.find_on_map{side=side}
 table.sort(own,function(a,b) return a.id<b.id end)
 local best,best_score={"end_turn"},0
 for _,unit in ipairs(own) do
  for _,target in ipairs(enemies) do
   if wesnoth.sides.is_enemy(side,target.side) then
    if distance(unit.x,unit.y,target.x,target.y)==1 and unit.attacks_left>0 then
     for i,w in ipairs(unit.attacks) do
      local att,def=wesnoth.simulate_combat(unit,i,target)
      local score=1000+400*(target.canrecruit and 1 or 0)+3*(target.hitpoints-def.average_hp)-2*(unit.hitpoints-att.average_hp)
      if unit.canrecruit and att.average_hp<unit.max_hitpoints*0.4 then score=score-200 end
      if score>best_score then best_score,best=score,{"attack",unit.x,unit.y,target.x,target.y,i-1} end
     end
    end
    if unit.moves>0 then
     for _,p in ipairs(wesnoth.paths.find_reach(unit)) do
      local d=distance(p.x,p.y,target.x,target.y)
      if not occupied[p.x..","..p.y] and d<distance(unit.x,unit.y,target.x,target.y) then
       local score=100-4*d+(target.canrecruit and 18 or 0)+(not unit.canrecruit and 30 or 0)
       if score>best_score then best_score,best=score,{"move",unit.x,unit.y,p.x,p.y} end
      end
     end
    end
   end
  end
 end
 return best
end

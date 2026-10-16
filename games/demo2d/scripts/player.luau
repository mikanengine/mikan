-- The classic scene's player as a Luau script (mikan-script; ADR 0011).
--
-- The engine calls `mikan_step` once per fixed step with the step duration in
-- seconds, whether each key is held, and the player's position in world
-- pixels. Return a direction (any length; longer than unit is normalized by
-- the engine) and a speed in pixels per second. The engine owns integration,
-- collision resolution, and world clamping; this script owns the decision.
--
-- This is the demo's real movement code: the same eight-direction, 420 px/s,
-- diagonals-not-faster behavior the Rust implementation used to have
-- (PLAYER_SPEED in games/demo2d/src/scene.rs documents the same number).

local SPEED = 420 -- pixels per second

function mikan_step(dt, left, right, up, down, pos_x, pos_y)
    local x = (right and 1 or 0) - (left and 1 or 0)
    local y = (down and 1 or 0) - (up and 1 or 0)
    if x == 0 and y == 0 then
        return 0, 0, 0
    end
    local length = math.sqrt(x * x + y * y)
    return x / length, y / length, SPEED
end

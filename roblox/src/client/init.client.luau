--!strict
-- Client: ask the server for vehicle bytes, build skinned MeshParts locally,
-- then demo the skinning by swinging the doors open.

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local VehicleLoader = require(ReplicatedStorage:WaitForChild("GtaVehicles"):WaitForChild("VehicleLoader"))

local remote = ReplicatedStorage:WaitForChild("GetVehicleData") :: RemoteFunction
local VEHICLE = "zion_hi"

local ok, result = pcall(function()
	local data = remote:InvokeServer(VEHICLE, false)
	return VehicleLoader.build(data, VEHICLE, workspace)
end)

if not ok then
	warn(`[GtaVehicle] failed to build {VEHICLE}: {result}`)
	warn("Check: HTTP requests enabled, backend running, SERVER_URL correct.")
	return
end

local model = result :: Model
model:PivotTo(CFrame.new(0, 10, 0))
print(`[GtaVehicle] built {VEHICLE}: {#model:GetChildren()} parts`)

-- Skinning demo: open the front doors. If the bones drive the mesh, the door
-- panels swing; if skinning didn't bind, the car just sits at rest (still fine).
task.wait(1.5)
VehicleLoader.poseBone(model, "door_dside_f", CFrame.Angles(0, math.rad(55), 0))
VehicleLoader.poseBone(model, "door_pside_f", CFrame.Angles(0, math.rad(-55), 0))
print("[GtaVehicle] posed doors open (skinning test)")

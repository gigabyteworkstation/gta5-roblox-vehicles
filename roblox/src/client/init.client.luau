--!strict
-- Client: ask the server for vehicle bytes, build the rigid parts + hinges,
-- then demo the HingeConstraints by opening the doors / hood / boot.

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local VehicleLoader = require(ReplicatedStorage:WaitForChild("GtaVehicles"):WaitForChild("VehicleLoader"))

local remote = ReplicatedStorage:WaitForChild("GetVehicleData") :: RemoteFunction
local VEHICLE = "zion_hi"

local ok, result = pcall(function()
	local data = remote:InvokeServer(VEHICLE, true) -- true = include textures
	return VehicleLoader.build(data, VEHICLE, workspace, CFrame.new(0, 8, 0))
end)

if not ok then
	warn(`[GtaVehicle] failed to build {VEHICLE}: {result}`)
	warn("Check: HTTP requests enabled, backend running, SERVER_URL correct.")
	return
end

local model = result :: Model
print(`[GtaVehicle] built {VEHICLE}: {#model:GetChildren()} parts`)

-- Demo the hinges: open everything to its GTA angle, then close, looping.
task.wait(1.5)
while model.Parent do
	VehicleLoader.openAll(model)
	print("[GtaVehicle] opened all (GTA angles)")
	task.wait(3)
	VehicleLoader.closeAll(model)
	task.wait(3)
end

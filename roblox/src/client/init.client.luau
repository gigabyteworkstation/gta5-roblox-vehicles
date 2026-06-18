--!strict
-- Client: ask the server for vehicle bytes, build the rigid parts + hinges,
-- then demo the HingeConstraints by opening the doors / hood / boot.

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local VehicleLoader = require(ReplicatedStorage:WaitForChild("GtaVehicles"):WaitForChild("VehicleLoader"))

local remote = ReplicatedStorage:WaitForChild("GetVehicleData") :: RemoteFunction
local VEHICLE = "zion_hi"

local ok, result = pcall(function()
	local data = remote:InvokeServer(VEHICLE, false)
	return VehicleLoader.build(data, VEHICLE, workspace, CFrame.new(0, 8, 0))
end)

if not ok then
	warn(`[GtaVehicle] failed to build {VEHICLE}: {result}`)
	warn("Check: HTTP requests enabled, backend running, SERVER_URL correct.")
	return
end

local model = result :: Model
print(`[GtaVehicle] built {VEHICLE}: {#model:GetChildren()} parts`)

-- Demo the hinges: open everything up.
task.wait(1.5)
VehicleLoader.setHinge(model, "door_dside_f", 60)
VehicleLoader.setHinge(model, "door_pside_f", 60)
VehicleLoader.setHinge(model, "bonnet", 45)
VehicleLoader.setHinge(model, "boot", 45)
print("[GtaVehicle] opened doors / hood / boot (hinge test)")

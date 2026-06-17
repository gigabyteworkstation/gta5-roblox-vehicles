--!strict
-- Client: ask the server for vehicle bytes, then build the MeshParts locally
-- (EditableMesh must be built client-side to be visible in Play).

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local VehicleLoader = require(ReplicatedStorage:WaitForChild("GtaVehicles"):WaitForChild("VehicleLoader"))

local remote = ReplicatedStorage:WaitForChild("GetVehicleData") :: RemoteFunction
local VEHICLE = "zion_hi"

local ok, result = pcall(function()
	local data = remote:InvokeServer(VEHICLE, false) -- pass true for textures (larger)
	return VehicleLoader.build(data, VEHICLE, workspace)
end)

if ok then
	local model = result :: Model
	model:PivotTo(CFrame.new(0, 10, 0))
	print(`[GtaVehicle] built {VEHICLE}: {#model:GetChildren()} parts`)
else
	warn(`[GtaVehicle] failed to build {VEHICLE}: {result}`)
	warn("Check: HTTP requests enabled, backend running, SERVER_URL correct.")
end

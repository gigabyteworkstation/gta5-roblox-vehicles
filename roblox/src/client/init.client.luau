--!strict
-- Client: ask the server for the vehicle bytes, build the physical car, and
-- leave it drivable. Walk up to the driver door, press the prompt to get in,
-- then WASD to drive, Space = handbrake, F = exit.

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
print("[GtaVehicle] Walk to the driver door and press the prompt to enter.")
print("[GtaVehicle] Drive: W/S throttle+brake, A/D steer, Space handbrake, F exit.")

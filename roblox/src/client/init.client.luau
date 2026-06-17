--!strict
-- Test client: fetches a vehicle from the backend and drops it in the world.
--
-- SETUP:
--   1. Game Settings → Security → enable "Allow HTTP Requests".
--   2. Set SERVER_URL to the machine running the Rust backend (`gtaveh serve`).
--      Same PC as Studio → http://127.0.0.1:5000
--      Another PC on your LAN → http://<that-pc-LAN-IP>:5000  (run serve on 0.0.0.0)

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local VehicleLoader = require(ReplicatedStorage.GtaVehicles.VehicleLoader)

local SERVER_URL = "http://127.0.0.1:5000"
local VEHICLE = "zion_hi"

local ok, result = pcall(function()
	return VehicleLoader.load(VEHICLE, SERVER_URL)
end)

if ok then
	local model = result :: Model
	model:PivotTo(CFrame.new(0, 5, 0))
	print(`[GtaVehicle] loaded {VEHICLE}: {#model:GetChildren()} parts`)
else
	warn(`[GtaVehicle] failed to load {VEHICLE}: {result}`)
	warn("Check: HTTP requests enabled, backend running, SERVER_URL correct.")
end

--!strict
-- Server proxy: clients can't use HttpService, so the server fetches the wire
-- bytes from the backend and hands them to the client over a RemoteFunction.
-- The client then builds the EditableMesh locally (server-built meshes don't
-- replicate).
--
-- SETUP:
--   1. Game Settings → Security → enable "Allow HTTP Requests".
--   2. Set SERVER_URL to the machine running the Rust backend (`gtaveh serve`).
--      Same PC as the backend → http://127.0.0.1:5000
--      Another PC on your LAN  → http://<that-pc-LAN-IP>:5000  (serve on 0.0.0.0)

local HttpService = game:GetService("HttpService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local SERVER_URL = "http://127.0.0.1:5000"

local remote = Instance.new("RemoteFunction")
remote.Name = "GetVehicleData"
remote.Parent = ReplicatedStorage

remote.OnServerInvoke = function(_player: Player, name: any, withTex: any): buffer
	assert(type(name) == "string", "vehicle name must be a string")
	local query = if withTex == true then "&tex=1" else ""
	local data = HttpService:GetAsync(`{SERVER_URL}/rpf/readVehicle?v={name}{query}`)
	return buffer.fromstring(data)
end

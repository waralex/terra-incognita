local M = {}

local data_dir = nil
local connections_file = nil

function M.setup(dir)
  data_dir = dir
  connections_file = dir .. "/connections.yml"
  vim.fn.mkdir(dir, "p")
end

function M.queries_dir(conn_name)
  local dir = data_dir .. "/queries/" .. conn_name
  vim.fn.mkdir(dir, "p")
  return dir
end

function M.load()
  if vim.fn.filereadable(connections_file) == 0 then
    return {}
  end
  local lines = vim.fn.readfile(connections_file)
  local conns = {}
  local current = nil
  for _, line in ipairs(lines) do
    local name = line:match("^%- name:%s*(.+)$")
    if name then
      current = { name = vim.trim(name) }
      table.insert(conns, current)
    end
    local port = line:match("^  port:%s*(%d+)$")
    if port and current then
      current.port = tonumber(port)
    end
  end
  return conns
end

function M.save(conns)
  local lines = {}
  for _, c in ipairs(conns) do
    table.insert(lines, "- name: " .. c.name)
    table.insert(lines, "  port: " .. tostring(c.port))
  end
  vim.fn.writefile(lines, connections_file)
end

function M.add(name, port)
  local conns = M.load()
  for _, c in ipairs(conns) do
    if c.name == name then
      vim.notify("Connection '" .. name .. "' already exists", vim.log.levels.WARN)
      return false
    end
  end
  table.insert(conns, { name = name, port = port })
  M.save(conns)
  M.queries_dir(name)
  return true
end

function M.remove(name)
  local conns = M.load()
  local new = {}
  for _, c in ipairs(conns) do
    if c.name ~= name then
      table.insert(new, c)
    end
  end
  M.save(new)
end

function M.find(name)
  for _, c in ipairs(M.load()) do
    if c.name == name then
      return c
    end
  end
  return nil
end

function M.list_queries(conn_name)
  local dir = M.queries_dir(conn_name)
  local files = vim.fn.globpath(dir, "*.yml", false, true)
  local names = {}
  for _, f in ipairs(files) do
    table.insert(names, vim.fn.fnamemodify(f, ":t"))
  end
  table.sort(names)
  return names
end

function M.query_path(conn_name, query_name)
  if not query_name:match("%.yml$") then
    query_name = query_name .. ".yml"
  end
  return M.queries_dir(conn_name) .. "/" .. query_name
end

function M.delete_query(conn_name, query_name)
  local path = M.query_path(conn_name, query_name)
  if vim.fn.filereadable(path) == 1 then
    vim.fn.delete(path)
    return true
  end
  return false
end

return M

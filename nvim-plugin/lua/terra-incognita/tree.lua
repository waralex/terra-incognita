local connection = require("terra-incognita.connection")

local M = {}

local expanded = {}
local tree_buf = nil
local tree_win = nil
local items = {}

function M.get_buf()
  return tree_buf
end

function M.get_win()
  return tree_win
end

function M.set_win(win)
  tree_win = win
end

local function build_items()
  items = {}
  local conns = connection.load()
  for _, conn in ipairs(conns) do
    local is_expanded = expanded[conn.name] or false
    table.insert(items, {
      type = "connection",
      name = conn.name,
      port = conn.port,
      expanded = is_expanded,
    })
    if is_expanded then
      local queries = connection.list_queries(conn.name)
      for _, q in ipairs(queries) do
        table.insert(items, {
          type = "query",
          name = q,
          conn_name = conn.name,
          conn_port = conn.port,
        })
      end
      table.insert(items, {
        type = "new_query",
        conn_name = conn.name,
        conn_port = conn.port,
      })
    end
  end
  return items
end

local function render_lines()
  local lines = {}
  for _, item in ipairs(items) do
    if item.type == "connection" then
      local icon = item.expanded and "▼" or "▶"
      table.insert(lines, icon .. " " .. item.name .. " :" .. tostring(item.port))
    elseif item.type == "query" then
      table.insert(lines, "    " .. item.name)
    elseif item.type == "new_query" then
      table.insert(lines, "    + new query")
    end
  end
  return lines
end

function M.create_buf()
  if tree_buf and vim.api.nvim_buf_is_valid(tree_buf) then
    return tree_buf
  end
  tree_buf = vim.api.nvim_create_buf(false, true)
  vim.bo[tree_buf].buftype = "nofile"
  vim.bo[tree_buf].swapfile = false
  vim.bo[tree_buf].filetype = "terra-tree"
  vim.api.nvim_buf_set_name(tree_buf, "terra://tree")
  M.setup_keymaps()
  return tree_buf
end

function M.refresh()
  if not tree_buf or not vim.api.nvim_buf_is_valid(tree_buf) then
    return
  end
  build_items()
  local lines = render_lines()
  vim.bo[tree_buf].modifiable = true
  vim.api.nvim_buf_set_lines(tree_buf, 0, -1, false, lines)
  vim.bo[tree_buf].modifiable = false
end

function M.get_item_at_cursor()
  if not tree_win or not vim.api.nvim_win_is_valid(tree_win) then
    return nil
  end
  local row = vim.api.nvim_win_get_cursor(tree_win)[1]
  return items[row]
end

function M.setup_keymaps()
  local opts = { buffer = tree_buf, silent = true, nowait = true }

  vim.keymap.set("n", "<CR>", function()
    local item = M.get_item_at_cursor()
    if not item then return end

    if item.type == "connection" then
      expanded[item.name] = not expanded[item.name]
      M.refresh()
    elseif item.type == "query" then
      local ui = require("terra-incognita.ui")
      ui.open_query(item.conn_name, item.conn_port, item.name)
    elseif item.type == "new_query" then
      vim.ui.input({ prompt = "Query name: " }, function(name)
        if not name or name == "" then return end
        if not name:match("%.yml$") then
          name = name .. ".yml"
        end
        local path = connection.query_path(item.conn_name, name)
        vim.fn.writefile({}, path)
        M.refresh()
        local ui = require("terra-incognita.ui")
        ui.open_query(item.conn_name, item.conn_port, name)
      end)
    end
  end, opts)

  vim.keymap.set("n", "d", function()
    local item = M.get_item_at_cursor()
    if not item or item.type ~= "query" then return end
    vim.ui.input({ prompt = "Delete " .. item.name .. "? (y/n): " }, function(answer)
      if answer == "y" then
        connection.delete_query(item.conn_name, item.name)
        M.refresh()
      end
    end)
  end, opts)

  vim.keymap.set("n", "a", function()
    vim.ui.input({ prompt = "Connection name: " }, function(name)
      if not name or name == "" then return end
      vim.ui.input({ prompt = "Port: " }, function(port_str)
        if not port_str or port_str == "" then return end
        local port = tonumber(port_str)
        if not port then
          vim.notify("Invalid port number", vim.log.levels.ERROR)
          return
        end
        if connection.add(name, port) then
          expanded[name] = true
          M.refresh()
        end
      end)
    end)
  end, opts)

  vim.keymap.set("n", "D", function()
    local item = M.get_item_at_cursor()
    if not item or item.type ~= "connection" then return end
    vim.ui.input({ prompt = "Delete connection '" .. item.name .. "'? (y/n): " }, function(answer)
      if answer == "y" then
        connection.remove(item.name)
        expanded[item.name] = nil
        M.refresh()
      end
    end)
  end, opts)

  vim.keymap.set("n", "q", function()
    local ui = require("terra-incognita.ui")
    ui.close()
  end, opts)
end

return M

local tree = require("terra-incognita.tree")
local executor = require("terra-incognita.executor")

local M = {}

local tree_win = nil
local query_win = nil
local result_win = nil
local result_buf = nil
local active_conn = nil

function M.is_open()
  return tree_win and vim.api.nvim_win_is_valid(tree_win)
end

function M.open()
  if M.is_open() then
    vim.api.nvim_set_current_win(tree_win)
    return
  end

  local prev_win = vim.api.nvim_get_current_win()

  vim.cmd("topleft vertical " .. 30 .. "split")
  tree_win = vim.api.nvim_get_current_win()
  local tree_buf = tree.create_buf()
  vim.api.nvim_win_set_buf(tree_win, tree_buf)
  tree.set_win(tree_win)
  vim.wo[tree_win].winfixwidth = true
  vim.wo[tree_win].number = false
  vim.wo[tree_win].relativenumber = false
  vim.wo[tree_win].signcolumn = "no"
  vim.wo[tree_win].cursorline = true

  tree.refresh()

  -- Restore focus if possible, otherwise stay in tree
  if vim.api.nvim_win_is_valid(prev_win) and prev_win ~= tree_win then
    vim.api.nvim_set_current_win(prev_win)
  end
end

function M.close()
  local wins = { tree_win, query_win, result_win }
  for _, w in ipairs(wins) do
    if w and vim.api.nvim_win_is_valid(w) then
      vim.api.nvim_win_close(w, true)
    end
  end
  tree_win = nil
  query_win = nil
  result_win = nil
end

function M.toggle()
  if M.is_open() then
    M.close()
  else
    M.open()
  end
end

local function ensure_result_buf()
  if result_buf and vim.api.nvim_buf_is_valid(result_buf) then
    return result_buf
  end
  result_buf = vim.api.nvim_create_buf(false, true)
  vim.bo[result_buf].buftype = "nofile"
  vim.bo[result_buf].swapfile = false
  vim.bo[result_buf].filetype = "yaml"
  vim.api.nvim_buf_set_name(result_buf, "terra://result")
  return result_buf
end

function M.open_query(conn_name, conn_port, query_name)
  local connection = require("terra-incognita.connection")
  local path = connection.query_path(conn_name, query_name)
  active_conn = { name = conn_name, port = conn_port }

  -- Close existing query/result windows
  for _, w in ipairs({ query_win, result_win }) do
    if w and vim.api.nvim_win_is_valid(w) then
      vim.api.nvim_win_close(w, true)
    end
  end

  -- Find a non-tree window or create one
  local target_win = nil
  for _, w in ipairs(vim.api.nvim_list_wins()) do
    if w ~= tree_win then
      target_win = w
      break
    end
  end

  if not target_win then
    vim.cmd("vertical split")
    target_win = vim.api.nvim_get_current_win()
  end

  -- Open query file in target window
  vim.api.nvim_set_current_win(target_win)
  vim.cmd("edit " .. vim.fn.fnameescape(path))
  query_win = target_win
  vim.bo.filetype = "yaml"

  -- Create result split to the right
  vim.cmd("vertical rightbelow split")
  result_win = vim.api.nvim_get_current_win()
  local rbuf = ensure_result_buf()
  vim.api.nvim_win_set_buf(result_win, rbuf)
  vim.wo[result_win].number = false
  vim.wo[result_win].relativenumber = false
  vim.wo[result_win].signcolumn = "no"

  -- Focus back on query
  vim.api.nvim_set_current_win(query_win)
end

function M.get_active_conn()
  return active_conn
end

function M.set_result(text)
  local buf = ensure_result_buf()
  vim.bo[buf].modifiable = true
  local lines = vim.split(text, "\n", { trimempty = false })
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false

  if result_win and vim.api.nvim_win_is_valid(result_win) then
    vim.api.nvim_win_set_cursor(result_win, { 1, 0 })
  end
end

function M.execute_current()
  if not active_conn then
    vim.notify("No active connection. Open a query first.", vim.log.levels.WARN)
    return
  end

  if not query_win or not vim.api.nvim_win_is_valid(query_win) then
    vim.notify("No query buffer open.", vim.log.levels.WARN)
    return
  end

  local query_buf = vim.api.nvim_win_get_buf(query_win)

  -- Save the buffer if modified
  if vim.bo[query_buf].modified then
    vim.api.nvim_buf_call(query_buf, function()
      vim.cmd("write")
    end)
  end

  local lines = vim.api.nvim_buf_get_lines(query_buf, 0, -1, false)
  local query_text = table.concat(lines, "\n")

  if query_text:match("^%s*$") then
    vim.notify("Query is empty.", vim.log.levels.WARN)
    return
  end

  M.set_result("# Executing...")

  vim.schedule(function()
    local output, _ = executor.execute(query_text, active_conn.port)
    M.set_result(output)
  end)
end

return M

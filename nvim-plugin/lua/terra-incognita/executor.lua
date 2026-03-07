local M = {}

local terra_cli = "terra-cli"

function M.setup(opts)
  terra_cli = opts.terra_cli or "terra-cli"
end

function M.execute(query_text, port)
  local url = "http://localhost:" .. tostring(port) .. "/query"
  local escaped = query_text:gsub("'", "'\\''")
  local cmd = string.format("echo '%s' | %s %s 2>&1", escaped, terra_cli, url)
  local output = vim.fn.system(cmd)
  local exit_code = vim.v.shell_error
  return output, exit_code
end

return M

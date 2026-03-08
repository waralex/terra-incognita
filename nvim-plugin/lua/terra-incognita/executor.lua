local M = {}

function M.setup(_) end

function M.execute(query_text, port)
  local url = "http://localhost:" .. tostring(port) .. "/query"
  local escaped = query_text:gsub("'", "'\\''")
  local cmd = string.format("curl -s -X POST -H 'Content-Type: application/yaml' -d '%s' '%s' 2>&1", escaped, url)
  local output = vim.fn.system(cmd)
  local exit_code = vim.v.shell_error
  return output, exit_code
end

return M

local M = {}

local defaults = {
  data_dir = vim.fn.expand("~/.terra-incognita/nvim"),
  keymap_execute = "<leader>te",
  keymap_toggle = "<leader>tt",
}

function M.setup(opts)
  opts = vim.tbl_deep_extend("force", defaults, opts or {})

  require("terra-incognita.connection").setup(opts.data_dir)

  vim.api.nvim_create_user_command("Terra", function()
    require("terra-incognita.ui").toggle()
  end, { desc = "Toggle Terra Incognita UI" })

  vim.api.nvim_create_user_command("TerraExecute", function()
    require("terra-incognita.ui").execute_current()
  end, { desc = "Execute current Terra query" })

  vim.keymap.set("n", opts.keymap_toggle, function()
    require("terra-incognita.ui").toggle()
  end, { desc = "Toggle Terra Incognita UI" })

  vim.keymap.set("n", opts.keymap_execute, function()
    require("terra-incognita.ui").execute_current()
  end, { desc = "Execute Terra query" })
end

return M

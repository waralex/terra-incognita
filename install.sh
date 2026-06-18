#!/usr/bin/env bash
# install.sh — build and install terra-incognita for the current user (no sudo).
#
# Interactive by default (asks about port, embeddings, model download, autostart).
# Pass --non-interactive (or any flag in a non-TTY) to use defaults/flags silently.
#
# Layout (XDG):
#   binary   ~/.local/bin/terra-server, ~/.local/bin/terractl
#   config   ~/.config/terra-incognita/{terra-server,project,schema}.yaml
#   data     ~/.local/share/terra-incognita/data        (RocksDB)
#   models   ~/.local/share/terra-incognita/models      (ONNX embeddings)
#   logs     ~/.local/share/terra-incognita/logs
#   agent    ~/Library/LaunchAgents/com.terra-incognita.server.plist (macOS)
#
# Flags (also act as non-interactive answers):
#   --port N  --onnx/--no-onnx  --model/--no-model
#   --autostart/--no-autostart  --host ADDR  --non-interactive
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BIN_DIR="$HOME/.local/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/terra-incognita"
DATA_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/terra-incognita"
DATA_DIR="$DATA_ROOT/data"
MODELS_DIR="$DATA_ROOT/models/all-MiniLM-L6-v2"
LOG_DIR="$DATA_ROOT/logs"

LABEL="com.terra-incognita.server"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
CONFIG_FILE="$CONFIG_DIR/terra-server.yaml"

# Defaults.
HOST="127.0.0.1"
PORT=7373
USE_ONNX=1
FETCH_MODEL=1
AUTOSTART=1
INTERACTIVE=1
[[ -t 0 ]] || INTERACTIVE=0

MODEL_BASE="https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --host)            [[ $# -ge 2 ]] || { echo "--host needs a value" >&2; exit 1; }; HOST="$2"; shift ;;
    --port)            [[ $# -ge 2 ]] || { echo "--port needs a value" >&2; exit 1; }; PORT="$2"; shift ;;
    --onnx)            USE_ONNX=1 ;;
    --no-onnx)         USE_ONNX=0; FETCH_MODEL=0 ;;
    --model)           FETCH_MODEL=1 ;;
    --no-model)        FETCH_MODEL=0 ;;
    --autostart)       AUTOSTART=1 ;;
    --no-autostart)    AUTOSTART=0 ;;
    --non-interactive) INTERACTIVE=0 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 1 ;;
  esac
  shift
done

[[ "$(uname)" == "Darwin" ]] || AUTOSTART=0

say()  { printf "\033[1;34m==>\033[0m %s\n" "$*"; }
warn() { printf "\033[1;33m!\033[0m %s\n" "$*"; }

ask_yesno() { # <question> <default 1|0> -> echoes 1|0
  local q="$1" cur="$2" def ans
  [[ "$cur" == "1" ]] && def="Y/n" || def="y/N"
  read -r -p "$q [$def] " ans </dev/tty || true
  [[ -z "$ans" ]] && { echo "$cur"; return; }
  case "$ans" in [Yy]*) echo 1 ;; *) echo 0 ;; esac
}

ask_value() { # <question> <default> -> echoes value
  local q="$1" cur="$2" ans
  read -r -p "$q [$cur] " ans </dev/tty || true
  [[ -z "$ans" ]] && echo "$cur" || echo "$ans"
}

# --- interactive prompts ---------------------------------------------------
if [[ $INTERACTIVE -eq 1 ]]; then
  echo "terra-incognita installer — press Enter to accept the [default]."
  echo
  PORT="$(ask_value "Port to listen on" "$PORT")"
  HOST="$(ask_value "Bind address (127.0.0.1 = localhost only; 0.0.0.0 = all interfaces, NO auth)" "$HOST")"
  USE_ONNX="$(ask_yesno "Enable embeddings (build with ONNX)?" "$USE_ONNX")"
  if [[ $USE_ONNX -eq 1 ]]; then
    FETCH_MODEL="$(ask_yesno "Download the all-MiniLM-L6-v2 model now (~90 MB)?" "$FETCH_MODEL")"
  else
    FETCH_MODEL=0
  fi
  if [[ "$(uname)" == "Darwin" ]]; then
    [[ "$HOST" == "0.0.0.0" ]] && warn "autostart + 0.0.0.0 means an always-on, unauthenticated server reachable on any network you join."
    AUTOSTART="$(ask_yesno "Start automatically at login (launchd)?" "$AUTOSTART")"
  fi
  echo
  say "Summary:"
  echo "  bind        $HOST:$PORT"
  echo "  embeddings  $([[ $USE_ONNX -eq 1 ]] && echo on || echo off)"
  echo "  model       $([[ $FETCH_MODEL -eq 1 ]] && echo download || echo skip)"
  echo "  autostart   $([[ $AUTOSTART -eq 1 ]] && echo yes || echo no)"
  echo
  [[ "$(ask_yesno "Proceed?" 1)" == "1" ]] || { echo "aborted"; exit 1; }
fi

# --- build -----------------------------------------------------------------
say "building terra-server (release${USE_ONNX:+, onnx})"
build_args=(build --release -p terra-server)
[[ $USE_ONNX -eq 1 ]] && build_args+=(--features onnx)
( cd "$REPO_DIR" && cargo "${build_args[@]}" )
BUILT_BIN="$REPO_DIR/target/release/terra-server"
[[ -x "$BUILT_BIN" ]] || { echo "build did not produce $BUILT_BIN" >&2; exit 1; }

# --- directories -----------------------------------------------------------
mkdir -p "$BIN_DIR" "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"
[[ $USE_ONNX -eq 1 ]] && mkdir -p "$MODELS_DIR"

# --- binaries --------------------------------------------------------------
say "installing binary to $BIN_DIR/terra-server"
install -m 0755 "$BUILT_BIN" "$BIN_DIR/terra-server"

say "installing terractl to $BIN_DIR/terractl"
sed \
  -e "s|@@TERRA_BIN@@|$BIN_DIR/terra-server|g" \
  -e "s|@@CONFIG_FILE@@|$CONFIG_FILE|g" \
  -e "s|@@DATA_DIR@@|$DATA_DIR|g" \
  -e "s|@@MODELS_DIR@@|$MODELS_DIR|g" \
  -e "s|@@LOG_DIR@@|$LOG_DIR|g" \
  -e "s|@@PLIST@@|$([[ $AUTOSTART -eq 1 ]] && echo "$PLIST")|g" \
  -e "s|@@LABEL@@|$LABEL|g" \
  "$REPO_DIR/scripts/terractl" >"$BIN_DIR/terractl"
chmod 0755 "$BIN_DIR/terractl"

# --- config (seed, never overwrite) ----------------------------------------
write_if_absent() {
  local path="$1"
  if [[ -e "$path" ]]; then
    say "keeping existing $path"
  else
    say "writing $path"
    cat >"$path"
  fi
}

embed_line=""
[[ $USE_ONNX -eq 1 ]] && embed_line="embed_model_dir: $MODELS_DIR"

write_if_absent "$CONFIG_FILE" <<EOF
host: $HOST
port: $PORT
project_config_path: ./project.yaml
$embed_line
EOF

write_if_absent "$CONFIG_DIR/project.yaml" <<EOF
data_dir: $DATA_DIR
schema_path: ./schema.yaml
EOF

if [[ ! -e "$CONFIG_DIR/schema.yaml" ]]; then
  say "writing $CONFIG_DIR/schema.yaml (from repo seed)"
  cp "$REPO_DIR/demo-terra-client/terra/schema.yaml" "$CONFIG_DIR/schema.yaml"
else
  say "keeping existing $CONFIG_DIR/schema.yaml"
fi

# --- model -----------------------------------------------------------------
if [[ $FETCH_MODEL -eq 1 ]]; then
  command -v curl >/dev/null || { echo "curl is required to download the model" >&2; exit 1; }
  fetch() {
    local url="$1" out="$2"
    if [[ -s "$out" ]]; then say "model file present: $out"; return; fi
    say "downloading $(basename "$out")"
    curl -fL --progress-bar "$url" -o "$out"
  }
  fetch "$MODEL_BASE/onnx/model.onnx" "$MODELS_DIR/model.onnx"
  fetch "$MODEL_BASE/tokenizer.json"  "$MODELS_DIR/tokenizer.json"
fi

# --- launch agent ----------------------------------------------------------
if [[ $AUTOSTART -eq 1 ]]; then
  say "installing launch agent $PLIST"
  mkdir -p "$(dirname "$PLIST")"
  cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key>
  <array><string>$BIN_DIR/terra-server</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>TERRA_SERVER_CONFIG</key><string>$CONFIG_FILE</string></dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$LOG_DIR/server.out.log</string>
  <key>StandardErrorPath</key><string>$LOG_DIR/server.err.log</string>
</dict>
</plist>
EOF
  launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$PLIST" || true
  launchctl kickstart -k "gui/$(id -u)/$LABEL" || true
fi

# --- shell env -------------------------------------------------------------
case "$(basename "${SHELL:-}")" in
  zsh)  RC="$HOME/.zshrc" ;;
  bash) RC="$HOME/.bashrc" ;;
  *)    RC="$HOME/.profile" ;;
esac

say "updating $RC (terra-incognita env block)"
tmp="$(mktemp)"
if [[ -f "$RC" ]]; then
  awk '/# >>> terra-incognita >>>/{skip=1} !skip{print} /# <<< terra-incognita <<</{skip=0}' "$RC" >"$tmp"
fi
{
  cat "$tmp"
  cat <<EOF
# >>> terra-incognita >>>
export PATH="\$HOME/.local/bin:\$PATH"
export TERRA_SERVER_CONFIG="$CONFIG_FILE"
# <<< terra-incognita <<<
EOF
} >"$RC.new"
mv "$RC.new" "$RC"
rm -f "$tmp"

say "done."
echo
echo "  bind     $HOST:$PORT"
echo "  binary   $BIN_DIR/terra-server"
echo "  config   $CONFIG_FILE"
echo "  data     $DATA_DIR"
[[ $USE_ONNX -eq 1 ]] && echo "  models   $MODELS_DIR"
echo
echo "Open a new shell (or 'source $RC'), then:"
echo "  terractl status      # check the server"
echo "  terractl drop-db     # wipe the database"
echo "  terractl uninstall   # remove (add --purge for config+data)"

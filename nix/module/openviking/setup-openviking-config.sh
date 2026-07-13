OPENVIKING_URL="${OPENVIKING_URL:-https://openviking.tail285d8f.ts.net}"
CONFIG_FILE="${OPENVIKING_CLI_CONFIG_FILE:-$HOME/.openviking/ovcli.conf}"
VAULT_MOUNT="secret"
VAULT_PATH="app/openviking"
VAULT_FIELD="root-api-key"

usage() {
    cat <<EOF
Usage: setup-openviking-config [--url <server url>]

Writes the OpenViking client config ($CONFIG_FILE) pointing at the
OpenViking server, fetching the API key from Vault ($VAULT_MOUNT/$VAULT_PATH).

Options:
  --url <url>   OpenViking server URL (default: $OPENVIKING_URL)
  -h, --help    Show this help

Environment:
  OPENVIKING_URL              Overrides the default server URL
  OPENVIKING_API_KEY          Skips the Vault lookup and uses this key
  OPENVIKING_CLI_CONFIG_FILE  Config file path (default: ~/.openviking/ovcli.conf)
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --url)
            OPENVIKING_URL="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
done

api_key="${OPENVIKING_API_KEY:-}"
if [ -z "$api_key" ]; then
    echo "Fetching OpenViking API key from Vault ($VAULT_MOUNT/$VAULT_PATH)..."
    if ! api_key="$(bao kv get -mount="$VAULT_MOUNT" -field="$VAULT_FIELD" "$VAULT_PATH")"; then
        echo "error: could not read $VAULT_FIELD from $VAULT_MOUNT/$VAULT_PATH." >&2
        echo "Log in first (e.g. 'bao login') or set OPENVIKING_API_KEY." >&2
        exit 1
    fi
fi

if [ -z "$api_key" ]; then
    echo "error: API key is empty." >&2
    exit 1
fi

mkdir -p "$(dirname "$CONFIG_FILE")"

if [ -f "$CONFIG_FILE" ]; then
    cp "$CONFIG_FILE" "$CONFIG_FILE.bak"
    echo "Existing config backed up to $CONFIG_FILE.bak"
fi

jq -n --arg url "$OPENVIKING_URL" --arg api_key "$api_key" \
    '{url: $url, api_key: $api_key, timeout: 60}' > "$CONFIG_FILE"
chmod 600 "$CONFIG_FILE"

echo "Wrote $CONFIG_FILE (url: $OPENVIKING_URL)"

if curl -fsS --max-time 10 "$OPENVIKING_URL/health" > /dev/null 2>&1; then
    echo "Server health check OK"
else
    echo "warning: could not reach $OPENVIKING_URL/health (is Tailscale up?)" >&2
fi

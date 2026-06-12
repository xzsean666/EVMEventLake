#!/usr/bin/env sh
set -eu

log() {
    printf '%s\n' "docker-ssh-tunnel: $*" >&2
}

fail() {
    log "$*"
    exit 1
}

is_false() {
    case "$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')" in
        0|false|no|off|disabled) return 0 ;;
        *) return 1 ;;
    esac
}

is_true() {
    case "$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')" in
        1|true|yes|on|enabled) return 0 ;;
        *) return 1 ;;
    esac
}

write_secret_file() {
    target_file="$1"
    secret_value="$2"
    umask 077
    # Accept both real newlines and env-friendly "\n" escaped newlines.
    printf '%b\n' "$secret_value" > "$target_file"
    chmod 0600 "$target_file"
}

start_ssh_tunnel() {
    command -v ssh >/dev/null 2>&1 || fail "ssh client is not installed in this image"

    : "${SSH_TUNNEL_HOST:?SSH_TUNNEL_HOST is required when SSH tunnel is enabled}"

    tunnel_user="${SSH_TUNNEL_USER:-root}"
    tunnel_port="${SSH_TUNNEL_PORT:-22}"
    socks_bind_host="${SSH_TUNNEL_SOCKS_BIND_HOST:-127.0.0.1}"
    socks_port="${SSH_TUNNEL_SOCKS_PORT:-1080}"
    connect_timeout="${SSH_TUNNEL_CONNECT_TIMEOUT_SECONDS:-15}"
    startup_wait="${SSH_TUNNEL_STARTUP_WAIT_SECONDS:-1}"
    strict_host_key_checking="${SSH_TUNNEL_STRICT_HOST_KEY_CHECKING:-accept-new}"
    work_dir="${SSH_TUNNEL_WORK_DIR:-/tmp/docker-ssh-tunnel}"

    mkdir -p "$work_dir"
    chmod 0700 "$work_dir"

    key_file="${SSH_TUNNEL_PRIVATE_KEY_FILE:-}"
    if [ -z "$key_file" ]; then
        key_file="$work_dir/id_tunnel"
        if [ -n "${SSH_TUNNEL_PRIVATE_KEY_B64:-}" ]; then
            printf '%s' "$SSH_TUNNEL_PRIVATE_KEY_B64" | base64 -d > "$key_file"
            chmod 0600 "$key_file"
        elif [ -n "${SSH_TUNNEL_PRIVATE_KEY:-}" ]; then
            write_secret_file "$key_file" "$SSH_TUNNEL_PRIVATE_KEY"
        elif [ -n "${SSH_AUTH_SOCK:-}" ]; then
            key_file=""
        else
            fail "set SSH_TUNNEL_PRIVATE_KEY_B64, SSH_TUNNEL_PRIVATE_KEY, SSH_TUNNEL_PRIVATE_KEY_FILE, or SSH_AUTH_SOCK"
        fi
    fi

    known_hosts_file="$work_dir/known_hosts"
    if [ -n "${SSH_TUNNEL_KNOWN_HOSTS:-}" ]; then
        write_secret_file "$known_hosts_file" "$SSH_TUNNEL_KNOWN_HOSTS"
        strict_host_key_checking="${SSH_TUNNEL_STRICT_HOST_KEY_CHECKING:-yes}"
    else
        touch "$known_hosts_file"
        chmod 0600 "$known_hosts_file"
    fi

    compression_flag=""
    if is_true "${SSH_TUNNEL_COMPRESSION:-false}"; then
        compression_flag="-C"
    fi

    log "opening SOCKS5 tunnel on ${socks_bind_host}:${socks_port} via ${tunnel_user}@${SSH_TUNNEL_HOST}:${tunnel_port}"
    if [ -n "$key_file" ]; then
        # shellcheck disable=SC2086
        ssh \
            $compression_flag \
            -N \
            -T \
            -D "${socks_bind_host}:${socks_port}" \
            -p "$tunnel_port" \
            -o ExitOnForwardFailure=yes \
            -o ServerAliveInterval="${SSH_TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS:-30}" \
            -o ServerAliveCountMax="${SSH_TUNNEL_SERVER_ALIVE_COUNT_MAX:-3}" \
            -o ConnectTimeout="$connect_timeout" \
            -o BatchMode=yes \
            -o StrictHostKeyChecking="$strict_host_key_checking" \
            -o UserKnownHostsFile="$known_hosts_file" \
            -i "$key_file" \
            "${tunnel_user}@${SSH_TUNNEL_HOST}" &
    else
        # shellcheck disable=SC2086
        ssh \
            $compression_flag \
            -N \
            -T \
            -D "${socks_bind_host}:${socks_port}" \
            -p "$tunnel_port" \
            -o ExitOnForwardFailure=yes \
            -o ServerAliveInterval="${SSH_TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS:-30}" \
            -o ServerAliveCountMax="${SSH_TUNNEL_SERVER_ALIVE_COUNT_MAX:-3}" \
            -o ConnectTimeout="$connect_timeout" \
            -o BatchMode=yes \
            -o StrictHostKeyChecking="$strict_host_key_checking" \
            -o UserKnownHostsFile="$known_hosts_file" \
            "${tunnel_user}@${SSH_TUNNEL_HOST}" &
    fi
    ssh_pid="$!"

    sleep "$startup_wait"
    if ! kill -0 "$ssh_pid" 2>/dev/null; then
        fail "ssh tunnel exited during startup"
    fi

    export SSH_TUNNEL_PID="$ssh_pid"
    export SSH_TUNNEL_PROXY_URL="${SSH_TUNNEL_PROXY_SCHEME:-socks5h}://${socks_bind_host}:${socks_port}"
}

export_proxy_environment() {
    proxy_url="$SSH_TUNNEL_PROXY_URL"
    default_no_proxy="127.0.0.1,localhost,::1,postgres,eventlake"
    no_proxy_value="${SSH_TUNNEL_NO_PROXY:-${NO_PROXY:-$default_no_proxy}}"

    export ALL_PROXY="$proxy_url"
    export all_proxy="$proxy_url"
    export HTTP_PROXY="$proxy_url"
    export http_proxy="$proxy_url"
    export HTTPS_PROXY="$proxy_url"
    export https_proxy="$proxy_url"
    export NO_PROXY="$no_proxy_value"
    export no_proxy="$no_proxy_value"

    log "proxy exported as ${proxy_url}; NO_PROXY=${no_proxy_value}"
}

cleanup() {
    status="${1:-0}"
    if [ -n "${app_pid:-}" ] && kill -0 "$app_pid" 2>/dev/null; then
        kill "$app_pid" 2>/dev/null || true
        wait "$app_pid" 2>/dev/null || true
    fi
    if [ -n "${SSH_TUNNEL_PID:-}" ] && kill -0 "$SSH_TUNNEL_PID" 2>/dev/null; then
        kill "$SSH_TUNNEL_PID" 2>/dev/null || true
        wait "$SSH_TUNNEL_PID" 2>/dev/null || true
    fi
    exit "$status"
}

if [ "$#" -eq 0 ]; then
    fail "usage: docker-ssh-tunnel-proxy.sh <command> [args...]"
fi

tunnel_enabled="${SSH_TUNNEL_ENABLED:-false}"
if is_false "$tunnel_enabled"; then
    exec "$@"
fi

if [ "$tunnel_enabled" = "auto" ]; then
    if [ -z "${SSH_TUNNEL_HOST:-}" ]; then
        exec "$@"
    fi
elif ! is_true "$tunnel_enabled"; then
    fail "SSH_TUNNEL_ENABLED must be true, false, or auto"
fi

start_ssh_tunnel
export_proxy_environment

trap 'cleanup 143' TERM INT
trap 'cleanup 129' HUP

"$@" &
app_pid="$!"

set +e
wait "$app_pid"
app_status="$?"
set -e
cleanup "$app_status"

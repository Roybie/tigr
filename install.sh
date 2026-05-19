#!/bin/sh
# Tigr installer. Detects your OS/arch, downloads the matching prebuilt
# binary from the latest GitHub Release, and places it on your PATH.
#
#   curl -fsSL https://roybie.github.io/tigr/install.sh | sh
#
# Environment overrides:
#   TIGR_VERSION   install a specific tag (e.g. v0.18.0) instead of latest
#   TIGR_BIN_DIR   install directory (default: ~/.tigr/bin)
set -eu

REPO="Roybie/tigr"
BIN_DIR="${TIGR_BIN_DIR:-$HOME/.tigr/bin}"

err() {
	echo "error: $*" >&2
	exit 1
}

# download <url> <dest> — curl if available, else wget.
download() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL "$1" -o "$2"
	elif command -v wget >/dev/null 2>&1; then
		wget -qO "$2" "$1"
	else
		err "need curl or wget to download tigr"
	fi
}

# ensure_path — add BIN_DIR to PATH via the detected shell's rc file.
ensure_path() {
	case ":$PATH:" in
	*":$BIN_DIR:"*)
		return 0
		;;
	esac

	case "$(basename "${SHELL:-}")" in
	zsh) rc="$HOME/.zshrc" ;;
	bash) rc="$HOME/.bashrc" ;;
	*) rc="$HOME/.profile" ;;
	esac

	line="export PATH=\"$BIN_DIR:\$PATH\""
	if [ -f "$rc" ] && grep -qF "$BIN_DIR" "$rc"; then
		: # already referenced — don't add it twice
	else
		printf '\n# Added by the tigr installer\n%s\n' "$line" >>"$rc"
		echo "Added $BIN_DIR to PATH in $rc"
	fi
	echo "Restart your shell, or run this now:  $line"
}

main() {
	# Map `uname` output to a Rust target triple.
	case "$(uname -s)" in
	Linux) os_part="unknown-linux-gnu" ;;
	Darwin) os_part="apple-darwin" ;;
	*) err "unsupported OS: $(uname -s) — build from source: https://github.com/$REPO" ;;
	esac

	case "$(uname -m)" in
	x86_64 | amd64) arch_part="x86_64" ;;
	arm64 | aarch64) arch_part="aarch64" ;;
	*) err "unsupported architecture: $(uname -m)" ;;
	esac

	target="${arch_part}-${os_part}"
	asset="tigr-${target}.tar.gz"

	version="${TIGR_VERSION:-latest}"
	if [ "$version" = "latest" ]; then
		url="https://github.com/$REPO/releases/latest/download/$asset"
	else
		url="https://github.com/$REPO/releases/download/$version/$asset"
	fi

	echo "Installing tigr ($target, $version)..."

	tmp="$(mktemp -d)"
	trap 'rm -rf "$tmp"' EXIT

	if ! download "$url" "$tmp/$asset"; then
		err "download failed: $url"
	fi
	tar -xzf "$tmp/$asset" -C "$tmp"
	[ -f "$tmp/tigr" ] || err "archive did not contain a 'tigr' binary"

	mkdir -p "$BIN_DIR"
	mv "$tmp/tigr" "$BIN_DIR/tigr"
	chmod +x "$BIN_DIR/tigr"

	echo "Installed tigr to $BIN_DIR/tigr"
	ensure_path
	echo "Run 'tigr --version' to verify."
}

main "$@"

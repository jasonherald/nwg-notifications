# Makefile for nwg-notifications — binary-repo subset per epic §3.6.
#
# Default install target is /usr/local (LSB convention for locally-built
# software). Contributors iterating from a clone should use the no-sudo
# override:
#   make install PREFIX=$HOME/.local BINDIR=$HOME/.cargo/bin
# Go-predecessor parity is an opt-in:
#   sudo make install PREFIX=/usr
#
# The `install-dbus` target is ALWAYS user-scope (no sudo). D-Bus user
# services are per-user by convention; installing the service file
# system-wide would break auto-activation for other users on the same
# machine. The per-user target auto-substitutes BIN_PATH into the
# committed service-file template.

CARGO   ?= cargo
PREFIX  ?= /usr/local
BINDIR  ?= $(PREFIX)/bin
DESTDIR ?=

BIN_NAME := nwg-notifications

# D-Bus user-service location — always user-scope regardless of PREFIX.
# XDG_DATA_HOME defaults to ~/.local/share.
DBUS_USER_DIR := $(HOME)/.local/share/dbus-1/services
DBUS_SERVICE_NAME := org.freedesktop.Notifications.service
DBUS_SERVICE_TEMPLATE := data/$(DBUS_SERVICE_NAME).in

SONAR_SCANNER ?= /opt/sonar-scanner/bin/sonar-scanner
SONAR_HOST_URL ?= https://sonar.aaru.network
SONAR_TRUSTSTORE ?= /tmp/sonar-truststore.jks
SONAR_TRUSTSTORE_PASSWORD ?= changeit

.PHONY: all build build-release test lint check-tools \
        lint-fmt lint-clippy lint-test lint-deny lint-audit \
        install install-bin install-dbus uninstall \
        upgrade \
        sonar clean help

all: build

define HELP_TEXT
Targets:
  make build           Debug build
  make build-release   Release build (used by install + upgrade)
  make test            cargo test + cargo clippy --all-targets
  make lint            Full local check: fmt + clippy + test + deny + audit
  make install         Build release + install binary (system-scope) + install-dbus (user-scope)
  make install-bin     Install binary to $(DESTDIR)$(BINDIR)
  make install-dbus    Install D-Bus service file to $(DBUS_USER_DIR) — ALWAYS user-scope (no sudo)
  make uninstall       Remove installed binary (system) and D-Bus service file (user)
  make upgrade         Resident-aware: capture running args, stop, rebuild, install, restart
  make sonar           Run SonarQube scan (requires sonar-scanner + .env)
  make clean           cargo clean

Install-path invocations:
  sudo make install                                              # default /usr/local
  make install PREFIX=$$HOME/.local BINDIR=$$HOME/.cargo/bin     # no-sudo dev
  sudo make install PREFIX=/usr                                  # distro-parity

Note: install-dbus runs unprivileged even under `sudo make install` —
it substitutes BIN_PATH into the service file template and drops it
into the invoking user's $(HOME)/.local/share/dbus-1/services/.
endef
export HELP_TEXT

help:
	@echo "$$HELP_TEXT"

build:
	$(CARGO) build

build-release:
	$(CARGO) build --release

test:
	$(CARGO) test
	$(CARGO) clippy --all-targets

check-tools:
	@if ! command -v cargo-deny >/dev/null 2>&1; then \
		echo "Installing cargo-deny..."; \
		$(CARGO) install cargo-deny; \
	fi
	@if ! command -v cargo-audit >/dev/null 2>&1; then \
		echo "Installing cargo-audit..."; \
		$(CARGO) install cargo-audit; \
	fi

lint-fmt:
	@echo "── Format ──"
	$(CARGO) fmt --all --check

lint-clippy:
	@echo "── Clippy ──"
	$(CARGO) clippy --all-targets -- -D warnings

lint-test:
	@echo "── Tests ──"
	$(CARGO) test

lint-deny:
	@echo "── Cargo Deny (licenses, advisories, bans, sources) ──"
	$(CARGO) deny check

lint-audit:
	@echo "── Cargo Audit (dependency CVEs) ──"
	$(CARGO) audit

lint: check-tools lint-fmt lint-clippy lint-test lint-deny lint-audit
	@echo ""
	@echo "All local checks passed ✓"

# ─────────────────────────────────────────────────────────────────────
# Install / uninstall
# ─────────────────────────────────────────────────────────────────────

install: build-release install-bin install-dbus

install-bin:
	@echo "Installing binary to $(DESTDIR)$(BINDIR)/$(BIN_NAME)"
	install -D -m 755 target/release/$(BIN_NAME) "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"

# install-dbus substitutes the @BIN_PATH@ placeholder in the template
# with the RUNTIME binary path ($(BINDIR)/$(BIN_NAME)). DESTDIR is
# intentionally excluded — it's the packager's staging directory
# (e.g. `make install DESTDIR=/tmp/pkg`) and baking it into D-Bus
# Exec= would activate a path that doesn't exist on the packaged
# system. The substituted service file is written into $(HOME)/.local/
# share/dbus-1/services/. Always user-scope — see Makefile header.
#
# If running under sudo with SUDO_USER set, we install to the ORIGINAL
# user's ~ instead of root's — the D-Bus session daemon running in
# the desktop user's session needs to find the service there.
install-dbus:
	@TARGET_HOME="$$HOME"; \
	if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
		TARGET_HOME="$$(getent passwd "$$SUDO_USER" | cut -d: -f6)"; \
		test -n "$$TARGET_HOME" || { \
			echo "ERROR: cannot resolve home directory for SUDO_USER=$$SUDO_USER"; \
			echo "  (getent passwd returned nothing — is $$SUDO_USER a real user on this system?)"; \
			exit 1; \
		}; \
		echo "sudo detected — installing D-Bus service file for user $$SUDO_USER (home: $$TARGET_HOME)"; \
	fi; \
	TARGET_DIR="$$TARGET_HOME/.local/share/dbus-1/services"; \
	TARGET_FILE="$$TARGET_DIR/$(DBUS_SERVICE_NAME)"; \
	BIN_PATH="$(BINDIR)/$(BIN_NAME)"; \
	echo "Installing D-Bus service file to $$TARGET_FILE"; \
	echo "  (D-Bus Exec path → $$BIN_PATH)"; \
	mkdir -p "$$TARGET_DIR" || exit 1; \
	sed "s|@BIN_PATH@|$$BIN_PATH|g" "$(DBUS_SERVICE_TEMPLATE)" > "$$TARGET_FILE" || exit 1; \
	if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
		chown "$$SUDO_USER:" "$$TARGET_FILE" "$$TARGET_DIR" || { \
			echo "ERROR: chown to $$SUDO_USER failed; D-Bus user-service would be unmanageable by the target user"; \
			exit 1; \
		}; \
	fi

uninstall:
	@echo "Removing binary"
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"
	@echo "Removing D-Bus service file"
	@TARGET_HOME="$$HOME"; \
	if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
		TARGET_HOME="$$(getent passwd "$$SUDO_USER" | cut -d: -f6)"; \
		test -n "$$TARGET_HOME" || { \
			echo "ERROR: cannot resolve home directory for SUDO_USER=$$SUDO_USER"; \
			echo "  (getent passwd returned nothing — D-Bus service file left behind;"; \
			echo "   remove manually from that user's ~/.local/share/dbus-1/services/)"; \
			exit 1; \
		}; \
	fi; \
	rm -f "$$TARGET_HOME/.local/share/dbus-1/services/$(DBUS_SERVICE_NAME)"
	@echo "Uninstalled."

# ─────────────────────────────────────────────────────────────────────
# Upgrade — daemon is resident in most configs, so capture + restart.
# ─────────────────────────────────────────────────────────────────────
#
# Linux-only: `pgrep` is procps-ng. nwg-notifications targets Hyprland +
# Sway (Linux Wayland compositors); cross-platform support out of scope.
#
# User-scoping: `pgrep -u $TARGET_USER -f $PGREP_PATTERN` restricts
# process discovery to the desktop user even when upgrade runs under
# sudo. The daemon is per-user (one D-Bus session per user); without
# -u we'd also match instances owned by other users on the same
# machine and SIGKILL them, which is never what the invoker wanted.
#
# -f (not -x): the kernel truncates /proc/PID/comm to 15 chars (TASK_
# COMM_LEN=16 incl. NUL), so `nwg-notifications` (17 chars) appears
# in comm as `nwg-notificatio`. `pgrep -x` matches against comm and
# would always miss the daemon. `pgrep -f` matches against the full
# /proc/PID/cmdline.
#
# The pattern anchors on `^` so it matches argv[0] only — NOT any
# occurrence of `/nwg-notifications` elsewhere in the cmdline. That
# matters because running `make upgrade` from the source checkout
# (`cd ~/source/nwg-notifications && make upgrade`) leaves the
# orchestrating bash shell with `/home/.../source/nwg-notifications`
# in its cmdline. The previous `(^|/)` alternation matched that
# substring and produced spurious pids, which then failed the
# --dump-args TOCTOU check and aborted the upgrade (see issue #4).
# `^([^[:space:]]+/)?$(BIN_NAME)(...)` requires the cmdline to
# START with either bare `nwg-notifications` or a path ending in
# `/nwg-notifications` — that's argv[0] by definition since pgrep -f
# matches the null-separated cmdline with nulls rendered as spaces.
#
# Root-refusal guard on the replay step: captured args come from the
# desktop user's process, replaying as root would start the daemon in
# the wrong user context (D-Bus sessions are per-user anyway).
upgrade: build-release
	@TARGET_USER="$${SUDO_USER:-$$(id -un)}"; \
	PGREP_PATTERN="^([^[:space:]]+/)?$(BIN_NAME)([[:space:]]|$$)"; \
	RUNNING_PIDS="$$(pgrep -u "$$TARGET_USER" -f "$$PGREP_PATTERN" 2>/dev/null || true)"; \
	if [ -n "$$RUNNING_PIDS" ]; then \
		ARGS_FILE="$$(mktemp)" || exit 1; \
		trap 'rm -f "$$ARGS_FILE"' EXIT; \
		for pid in $$RUNNING_PIDS; do \
			target/release/$(BIN_NAME) --dump-args "$$pid" >> "$$ARGS_FILE" || exit 1; \
		done; \
		echo "Running daemon(s) for $$TARGET_USER: $$RUNNING_PIDS — stopping before install"; \
		kill $$RUNNING_PIDS 2>/dev/null || true; \
		sleep 1; \
		STILL_RUNNING="$$(pgrep -u "$$TARGET_USER" -f "$$PGREP_PATTERN" 2>/dev/null || true)"; \
		if [ -n "$$STILL_RUNNING" ]; then \
			echo "Warning: still running after SIGTERM: $$STILL_RUNNING — escalating to SIGKILL"; \
			kill -9 $$STILL_RUNNING 2>/dev/null || true; \
			sleep 1; \
			STILL_RUNNING="$$(pgrep -u "$$TARGET_USER" -f "$$PGREP_PATTERN" 2>/dev/null || true)"; \
			test -z "$$STILL_RUNNING" || { \
				echo "ERROR: failed to stop $$STILL_RUNNING after SIGKILL; aborting install to avoid file-in-use"; \
				exit 1; \
			}; \
		fi; \
		$(MAKE) install-bin install-dbus || exit 1; \
		if [ -s "$$ARGS_FILE" ]; then \
			if [ "$$(id -u)" -eq 0 ]; then \
				echo "Refusing to replay captured daemon args as root — D-Bus sessions"; \
				echo "are per-user; running the daemon in root context won't receive"; \
				echo "notifications from the desktop session anyway. Install finished;"; \
				echo "restart the daemon manually from your desktop session (or let"; \
				echo "D-Bus auto-activate it on the next notify-send)."; \
			else \
				while IFS= read -r args; do \
					echo "Restarting with captured args: $$args"; \
					setsid sh -c "$$args" </dev/null >/dev/null 2>&1 & \
				done < "$$ARGS_FILE"; \
			fi; \
		fi; \
	else \
		echo "No running daemon for $$TARGET_USER — installing; next notify-send D-Bus-activates the new binary"; \
		$(MAKE) install-bin install-dbus || exit 1; \
	fi
	@echo "Upgrade complete."

# ─────────────────────────────────────────────────────────────────────
# SonarQube scan — .env is PARSED (never sourced) to avoid shell injection.
# ─────────────────────────────────────────────────────────────────────

sonar:
	@echo "Running SonarQube scan..."
	@test -f ./.env || { echo "ERROR: .env not found in repo root"; exit 1; }
	@command -v "$(SONAR_SCANNER)" >/dev/null 2>&1 || [ -x "$(SONAR_SCANNER)" ] || { \
		echo "ERROR: sonar-scanner not found (looked at $(SONAR_SCANNER))"; exit 1; \
	}
	@test -r "$(SONAR_TRUSTSTORE)" || { \
		echo "ERROR: truststore not found or not readable at $(SONAR_TRUSTSTORE)"; \
		echo "  (sonar.aaru.network uses a self-signed cert — regenerate with:"; \
		echo "     openssl s_client -connect sonar.aaru.network:443 -showcerts </dev/null 2>/dev/null \\\\"; \
		echo "       | awk '/BEGIN CERT/,/END CERT/' > /tmp/sonar-cert.pem && \\\\"; \
		echo "     keytool -importcert -alias sonar-aaru -file /tmp/sonar-cert.pem \\\\"; \
		echo "       -keystore $(SONAR_TRUSTSTORE) -storepass $(SONAR_TRUSTSTORE_PASSWORD) -noprompt)"; \
		exit 1; \
	}
	@TOKEN="$$(awk '/^SONAR_TOKEN=/{sub(/^[^=]*=[ \t]*/, ""); sub(/[ \t]+$$/, ""); print; exit}' ./.env)"; \
	test -n "$$TOKEN" || { echo "ERROR: SONAR_TOKEN is empty in .env"; exit 1; }; \
	SONAR_TOKEN="$$TOKEN" \
	SONAR_SCANNER_OPTS="-Djavax.net.ssl.trustStore=$(SONAR_TRUSTSTORE) -Djavax.net.ssl.trustStorePassword=$(SONAR_TRUSTSTORE_PASSWORD)" \
	"$(SONAR_SCANNER)" -Dsonar.host.url="$(SONAR_HOST_URL)"

clean:
	$(CARGO) clean

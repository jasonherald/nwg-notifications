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
# Two service files ship: the standard freedesktop notification interface
# (auto-activated when an app calls `Notify`), and the project-private
# `org.nwg.Notifications` count IPC (auto-activated when nwg-panel queries
# the count badge on cold boot — see #65 for context).
DBUS_SERVICE_NAMES := org.freedesktop.Notifications.service org.nwg.Notifications.service

SONAR_SCANNER ?= /opt/sonar-scanner/bin/sonar-scanner
SONAR_HOST_URL ?= https://sonar.aaru.network
SONAR_TRUSTSTORE ?= /tmp/sonar-truststore.jks
SONAR_TRUSTSTORE_PASSWORD ?= changeit

.PHONY: all build build-release test lint check-tools \
        lint-fmt lint-clippy lint-test lint-deny lint-audit \
        install install-bin install-dbus uninstall uninstall-dbus \
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
  make install-dbus    Install D-Bus service files to $(DBUS_USER_DIR) — ALWAYS user-scope (no sudo)
  make uninstall       Remove installed binary (system) and D-Bus service files (user)
  make uninstall-dbus  Remove D-Bus service files only (user) — symmetric with install-dbus
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
		echo "sudo detected — installing D-Bus service files for user $$SUDO_USER (home: $$TARGET_HOME)"; \
	fi; \
	TARGET_DIR="$$TARGET_HOME/.local/share/dbus-1/services"; \
	BIN_PATH="$(BINDIR)/$(BIN_NAME)"; \
	mkdir -p "$$TARGET_DIR" || exit 1; \
	for SERVICE_NAME in $(DBUS_SERVICE_NAMES); do \
		TEMPLATE="data/$$SERVICE_NAME.in"; \
		TARGET_FILE="$$TARGET_DIR/$$SERVICE_NAME"; \
		echo "Installing D-Bus service file to $$TARGET_FILE"; \
		echo "  (D-Bus Exec path → $$BIN_PATH)"; \
		sed "s|@BIN_PATH@|$$BIN_PATH|g" "$$TEMPLATE" > "$$TARGET_FILE" || exit 1; \
		if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
			chown "$$SUDO_USER:" "$$TARGET_FILE" || { \
				echo "ERROR: chown $$TARGET_FILE to $$SUDO_USER failed; D-Bus user-service would be unmanageable by the target user"; \
				exit 1; \
			}; \
		fi; \
	done; \
	if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
		chown "$$SUDO_USER:" "$$TARGET_DIR" || { \
			echo "ERROR: chown $$TARGET_DIR to $$SUDO_USER failed"; \
			exit 1; \
		}; \
	fi

# uninstall-dbus mirrors install-dbus: removes every service file
# install-dbus would lay down. Symmetric so packagers + scripts can
# clean up D-Bus state without removing the binary (and vice versa).
uninstall-dbus:
	@echo "Removing D-Bus service files"
	@TARGET_HOME="$$HOME"; \
	if [ -n "$$SUDO_USER" ] && [ "$$(id -u)" -eq 0 ]; then \
		TARGET_HOME="$$(getent passwd "$$SUDO_USER" | cut -d: -f6)"; \
		test -n "$$TARGET_HOME" || { \
			echo "ERROR: cannot resolve home directory for SUDO_USER=$$SUDO_USER"; \
			echo "  (getent passwd returned nothing — D-Bus service files left behind;"; \
			echo "   remove manually from that user's ~/.local/share/dbus-1/services/)"; \
			exit 1; \
		}; \
	fi; \
	for SERVICE_NAME in $(DBUS_SERVICE_NAMES); do \
		TARGET_FILE="$$TARGET_HOME/.local/share/dbus-1/services/$$SERVICE_NAME"; \
		rm -f "$$TARGET_FILE" || { \
			echo "ERROR: failed to remove $$TARGET_FILE — refusing to continue with a stale D-Bus state"; \
			exit 1; \
		}; \
	done

uninstall: uninstall-dbus
	@echo "Removing binary"
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN_NAME)"
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
# Install-target validation (issue #5): before killing anything, resolve
# /proc/$PID/exe for each running daemon and compare against where this
# upgrade would install ($(BINDIR)/$(BIN_NAME)). If they don't match —
# usually because the user installed to ~/.cargo/bin but invoked upgrade
# without re-passing PREFIX/BINDIR, so we'd try to install to /usr/local
# and fail on permission — we abort with a helpful error BEFORE touching
# the daemon. Previously the recipe killed the daemon first and then
# failed the install, leaving the desktop session with a silently dead
# notification daemon.
#
# Atomicity (issue #5): recipe order is validate → capture args → install
# → kill → restart. Install happens while the daemon is still running
# (Linux's mmap semantics mean replacing the binary file via `install`'s
# unlink+write doesn't disturb the running process's loaded pages). If
# install fails, the daemon is never killed and the user sees a clear
# error.
#
# PID identity validation (CodeRabbit follow-up on #5): we capture
# `/proc/$PID/stat` field 22 (starttime — clock ticks since boot) when
# we first touch each pid, and re-verify it matches before sending
# SIGTERM and before sending SIGKILL. Linux reuses pids after pid_max
# wraps; though vanishingly unlikely on a desktop in a sub-second
# window, it's cheap to defend against — without this check a reused
# pid could point at an unrelated process by the time `kill` fires.
# Starttime is kernel-authoritative and unique per (pid, boot) pair.
#
# --dump-args failure handling (CodeRabbit follow-up on #5): a failure
# is only swallowed when the pid has actually disappeared (no
# `/proc/$PID/exe`). If --dump-args fails on a still-live daemon that's
# a real bug that'd leave us with empty args + a killed daemon, so we
# fail-fast with an error.
#
# Root-refusal guard on the replay step: captured args come from the
# desktop user's process, replaying as root would start the daemon in
# the wrong user context (D-Bus sessions are per-user anyway).
upgrade: build-release
	@TARGET_USER="$${SUDO_USER:-$$(id -un)}"; \
	PGREP_PATTERN="^([^[:space:]]+/)?$(BIN_NAME)([[:space:]]|$$)"; \
	RUNNING_PIDS="$$(pgrep -u "$$TARGET_USER" -f "$$PGREP_PATTERN" 2>/dev/null || true)"; \
	if [ -n "$$RUNNING_PIDS" ]; then \
		INSTALL_TARGET="$(DESTDIR)$(BINDIR)/$(BIN_NAME)"; \
		INSTALL_TARGET_REAL="$$(readlink -f "$$INSTALL_TARGET" 2>/dev/null || echo "$$INSTALL_TARGET")"; \
		for pid in $$RUNNING_PIDS; do \
			RUNNING_EXE="$$(readlink -f "/proc/$$pid/exe" 2>/dev/null)"; \
			if [ -z "$$RUNNING_EXE" ]; then \
				if [ -d "/proc/$$pid" ]; then \
					echo "ERROR: unable to resolve /proc/$$pid/exe for live daemon pid $$pid"; \
					echo "       (process is alive but its exe symlink is unreadable — refusing to proceed"; \
					echo "        without install-target validation)"; \
					exit 1; \
				fi; \
				continue; \
			fi; \
			if [ "$$RUNNING_EXE" != "$$INSTALL_TARGET_REAL" ]; then \
				RUNNING_BINDIR="$$(dirname "$$RUNNING_EXE")"; \
				echo "ERROR: running daemon (pid $$pid) is installed at"; \
				echo "         $$RUNNING_EXE"; \
				echo "       but 'make upgrade' would install to"; \
				echo "         $$INSTALL_TARGET"; \
				echo ""; \
				echo "       Daemon NOT killed — a prefix-mismatched upgrade would leave"; \
				echo "       you with a dead notification daemon and no new binary."; \
				echo ""; \
				echo "       Re-run with BINDIR matching the running binary:"; \
				echo "         make upgrade BINDIR=$$RUNNING_BINDIR"; \
				echo "       (install-dbus is always user-scope — PREFIX is ignored here"; \
				echo "       because the D-Bus service file always lands in ~/.local.)"; \
				exit 1; \
			fi; \
		done; \
		ARGS_FILE="$$(mktemp)" || exit 1; \
		RUNNING_INFO="$$(mktemp)" || exit 1; \
		trap 'rm -f "$$ARGS_FILE" "$$RUNNING_INFO"' EXIT; \
		for pid in $$RUNNING_PIDS; do \
			START_TIME="$$(sed 's/.*) //' "/proc/$$pid/stat" 2>/dev/null | awk '{print $$20}' || true)"; \
			test -n "$$START_TIME" || continue; \
			if ! DUMP_OUT="$$(target/release/$(BIN_NAME) --dump-args "$$pid" 2>/dev/null)"; then \
				ACTUAL_START="$$(sed 's/.*) //' "/proc/$$pid/stat" 2>/dev/null | awk '{print $$20}' || true)"; \
				ACTUAL_EXE="$$(readlink -f "/proc/$$pid/exe" 2>/dev/null || true)"; \
				if [ -n "$$ACTUAL_START" ] && [ "$$ACTUAL_START" = "$$START_TIME" ] && \
				   [ "$$ACTUAL_EXE" = "$$INSTALL_TARGET_REAL" ]; then \
					echo "ERROR: --dump-args failed for live daemon pid $$pid"; \
					exit 1; \
				fi; \
				continue; \
			fi; \
			printf "%s\t%s\n" "$$pid" "$$DUMP_OUT" >> "$$ARGS_FILE" || exit 1; \
			echo "$$pid $$START_TIME" >> "$$RUNNING_INFO" || exit 1; \
		done; \
		$(MAKE) install-bin install-dbus || exit 1; \
		VALIDATED_PIDS=""; \
		while IFS=' ' read -r pid start_time; do \
			ACTUAL_START="$$(sed 's/.*) //' "/proc/$$pid/stat" 2>/dev/null | awk '{print $$20}' || true)"; \
			if [ -n "$$ACTUAL_START" ] && [ "$$ACTUAL_START" = "$$start_time" ]; then \
				kill "$$pid" 2>/dev/null || true; \
				VALIDATED_PIDS="$$VALIDATED_PIDS $$pid"; \
			else \
				echo "Skipping pid $$pid — no longer our daemon (starttime changed or process exited between capture and kill)"; \
			fi; \
		done < "$$RUNNING_INFO"; \
		if [ -n "$$VALIDATED_PIDS" ]; then \
			echo "Sent SIGTERM to daemon(s) for $$TARGET_USER:$$VALIDATED_PIDS"; \
			sleep 1; \
			STILL_RUNNING=""; \
			for pid in $$VALIDATED_PIDS; do \
				START_TIME="$$(grep "^$$pid " "$$RUNNING_INFO" | awk '{print $$2}')"; \
				ACTUAL_START="$$(sed 's/.*) //' "/proc/$$pid/stat" 2>/dev/null | awk '{print $$20}' || true)"; \
				if [ -n "$$ACTUAL_START" ] && [ "$$ACTUAL_START" = "$$START_TIME" ]; then \
					kill -9 "$$pid" 2>/dev/null || true; \
					STILL_RUNNING="$$STILL_RUNNING $$pid"; \
				fi; \
			done; \
			if [ -n "$$STILL_RUNNING" ]; then \
				echo "Escalated to SIGKILL:$$STILL_RUNNING"; \
				sleep 1; \
				FINAL_ALIVE=""; \
				for pid in $$STILL_RUNNING; do \
					START_TIME="$$(grep "^$$pid " "$$RUNNING_INFO" | awk '{print $$2}')"; \
					ACTUAL_START="$$(sed 's/.*) //' "/proc/$$pid/stat" 2>/dev/null | awk '{print $$20}' || true)"; \
					if [ -n "$$ACTUAL_START" ] && [ "$$ACTUAL_START" = "$$START_TIME" ]; then \
						FINAL_ALIVE="$$FINAL_ALIVE $$pid"; \
					fi; \
				done; \
				test -z "$$FINAL_ALIVE" || { \
					echo "ERROR: failed to stop$$FINAL_ALIVE after SIGKILL; binary installed but daemon still holds old mmap"; \
					exit 1; \
				}; \
			fi; \
		fi; \
		if [ -n "$$VALIDATED_PIDS" ] && [ -s "$$ARGS_FILE" ]; then \
			if [ "$$(id -u)" -eq 0 ]; then \
				echo "Refusing to replay captured daemon args as root — D-Bus sessions"; \
				echo "are per-user; running the daemon in root context won't receive"; \
				echo "notifications from the desktop session anyway. Install finished;"; \
				echo "restart the daemon manually from your desktop session (or let"; \
				echo "D-Bus auto-activate it on the next notify-send)."; \
			else \
				for pid in $$VALIDATED_PIDS; do \
					args="$$(awk -v p="$$pid" 'BEGIN{FS="\t"} $$1==p{sub(/^[^\t]*\t/, ""); print; exit}' "$$ARGS_FILE")"; \
					test -n "$$args" || continue; \
					echo "Restarting with captured args: $$args"; \
					setsid sh -c "$$args" </dev/null >/dev/null 2>&1 & \
				done; \
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

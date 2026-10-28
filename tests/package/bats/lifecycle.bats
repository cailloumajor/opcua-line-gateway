# Package lifecycle: fresh installation, operation, removal and purge.

load helpers

@test "package installs with dpkg, without pulling any other package" {
    # dpkg neither resolves dependencies nor installs recommended packages, so
    # an undeclared dependency missing from the image makes this fail.
    run dpkg --install /pkg/testpkg.deb
    assert_success
}

@test "system user is created" {
    run getent passwd "$SVC_USER"
    assert_success
}

@test "service is enabled, to start on boot" {
    run systemctl is-enabled "$SVC"
    assert_output enabled
}

@test "service unit passes systemd verification" {
    # Catches unknown directives, a missing binary, invalid values...
    run systemd-analyze verify "/usr/lib/systemd/system/${SVC}.service"
    assert_success
}

@test "configuration examples are shipped" {
    assert_file_exists "${EXAMPLES_DIR}/config.toml"
    assert_file_exists "${EXAMPLES_DIR}/credentials.toml"
}

@test "service does not start without configuration" {
    systemctl start "$SVC"

    run systemctl show --property ConditionResult --value "$SVC"
    assert_output no
    run systemctl is-active "$SVC"
    assert_output inactive
}

@test "configured gateway starts and writes to the database" {
    # As documented in the README.
    install --directory "$CONFIG_DIR"
    install --mode 0644 /fixtures/config.toml "$CONFIG_DIR/"
    install --mode 0600 /fixtures/credentials.toml "$CREDENTIALS_FILE"
    systemctl start "$SVC"
    wait_log "$LOG_FILE" "$READY_MSG"

    run systemctl is-active "$SVC"
    assert_output active
}

@test "gateway gets the system timezone" {
    assert_file_contains "$LOG_FILE" "got system timezone"
}

@test "cache file is created in the state directory" {
    # Relative paths in the configuration resolve against the working
    # directory, i.e. the state directory.
    run compgen -G "${STATE_DIR}/*.redb"
    assert_success
}

@test "log file is readable by everyone" {
    # Mode is set by the service, when (re)creating the file.
    assert_file_owner "$SVC_USER" "$LOG_FILE"
    assert_file_permission 644 "$LOG_FILE"
}

@test "log rotation signals the gateway" {
    # Forced rotation renames the log file, then reloads the service, which
    # sends SIGHUP to the gateway. The signal is logged before reopening, so in
    # the rotated file.
    logrotate --force "$LOGROTATE_FILE"

    assert_file_exists "${LOG_FILE}.1"
    wait_log "${LOG_FILE}" "received SIGHUP signal"
}

@test "recreated log file is readable by everyone" {
    # Mode is set by the service, when (re)creating the file.
    assert_file_owner "$SVC_USER" "$LOG_FILE"
    assert_file_permission 644 "$LOG_FILE"
}

@test "service stops gracefully, logging to the reopened file" {
    systemctl stop "$SVC"

    # The termination is logged in the new file only if the gateway reopened
    # it after rotation.
    assert_graceful_stop
}

@test "removal deletes the binary and the unit" {
    run dpkg --remove "$SVC"
    assert_success

    assert_file_not_exists "/usr/bin/${SVC}"
    assert_file_not_exists "/usr/lib/systemd/system/${SVC}.service"
}

@test "removal keeps configuration files until purge" {
    assert_file_exists "$LOGROTATE_FILE"
}

@test "purge deletes configuration files" {
    run dpkg --purge "$SVC"
    assert_success

    assert_file_not_exists "$LOGROTATE_FILE"
    run systemctl show --property LoadState --value "$SVC"
    assert_output not-found
}

@test "purge keeps administrator and runtime files, and the system user" {
    # Like for any Debian package, files not shipped by the package are left
    # in place, as is the system user.
    assert_file_exists "${CONFIG_DIR}/config.toml"
    assert_file_exists "$CREDENTIALS_FILE"
    assert_dir_exists "$STATE_DIR"
    assert_dir_exists "$LOGS_DIR"
    run getent passwd "$SVC"
    assert_success
}

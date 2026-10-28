# Common definitions for the Debian package tests, run with bats inside the
# systemd container once it has booted.
#
# Expected files:
# - /pkg/opcua-line-gateway.deb: the package under test;
# - /fixtures/config.toml and /fixtures/credentials.toml: gateway
#   configuration, pointing to the ClickHouse service, without any machine.

bats_load_library bats-support
bats_load_library bats-assert
bats_load_library bats-file

SVC=opcua-line-gateway
# shellcheck disable=SC2034  # Constant is used by the test files.
SVC_USER=opcua-line-gateway
# shellcheck disable=SC2034  # Constant is used by the test files.
CONFIG_DIR=/etc/${SVC}
# shellcheck disable=SC2034  # Constant is used by the test files.
CREDENTIALS_FILE=/etc/credstore/${SVC}.toml
# shellcheck disable=SC2034  # Constant is used by the test files.
EXAMPLES_DIR=/usr/share/doc/${SVC}/examples
# shellcheck disable=SC2034  # Constant is used by the test files.
STATE_DIR=/var/lib/${SVC}
LOGS_DIR=/var/log/${SVC}
LOG_FILE=${LOGS_DIR}/${SVC}.log
# shellcheck disable=SC2034  # Constant is used by the test files.
LOGROTATE_FILE=/etc/logrotate.d/${SVC}

# Logged once the gateway has written to the database, i.e. it is fully
# operational.
# shellcheck disable=SC2034  # Constant is used by the test files.
READY_MSG="first draining task run was successful"
# Logged when the gateway handles a termination signal.
TERMINATION_MSG="received termination signal, shutting down"

teardown() {
    if [[ -z ${BATS_TEST_COMPLETED:-} && -z ${BATS_TEST_SKIPPED:-} ]]; then
        print_diagnostics
    fi
}

# Prints the service state and logs, shown by bats along with the failed test
# output.
print_diagnostics() {
    echo "--- service status"
    systemctl status --no-pager "$SVC" || true
    echo "--- service journal"
    journalctl --no-pager --unit "$SVC" || true
    echo "--- log files"
    tail -n 50 "$LOG_FILE"* || true
}

# Waits for a file to contain a fixed string at least a given number of times.
#
# Arguments:
#   $1 - file path
#   $2 - fixed string
#   $3 - minimum occurrences count, defaults to 1
wait_log() {
    # Not `readonly`, which would make variables global and break bats
    # internals using the same names.
    local -r file=$1 pattern=$2 count=${3:-1}
    local found
    for _ in $(seq 30); do
        found=$(grep --count --fixed-strings -- "$pattern" "$file" 2>/dev/null) || found=0
        if ((found >= count)); then
            return 0
        fi
        sleep 1
    done
    fail "timed out waiting for '$pattern' (x$count) in $file"
}

# Asserts that the service has been stopped gracefully: systemd reports
# neither a timeout nor a signal, and the gateway logged its shutdown.
assert_graceful_stop() {
    run systemctl show --property Result --value "$SVC"
    assert_output success
    assert_file_contains "$LOG_FILE" "$TERMINATION_MSG"
}

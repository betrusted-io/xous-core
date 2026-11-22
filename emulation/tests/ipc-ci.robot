*** Comments ***
xous-ipc suite for `renode-test` (the CI path).

Adapted from emulation/tests/std-net.robot for the ipc-ci image
(xtask ipc-ci: base_pkgs + ipc-echo + ipc-tests). Like the net suite there
is no first-boot UX to drive: no PDDB, no keyboard, no graphics, and no net
either -- the boot milestone sequence is just the banner followed directly by
the sentinel stream. tools/ipc-ci.py is the reference driver; the two must be
kept in lockstep (and ipc-ci.resc ends with `start`, so the terminal tester
can be created right after the include).

Timeouts are VIRTUAL-time seconds; calibrate after the first green runs.

*** Settings ***
Documentation                 Boot betrusted-soc under Renode, wait for the
...                           boot banner, then assert on the ipc-tests
...                           sentinel stream.
Suite Setup                   Setup
Suite Teardown                Teardown
Test Teardown                 Test Teardown
Resource                      ${RENODEKEYWORDS}
Library                       OperatingSystem

*** Variables ***
${IMAGE_DIR}                  ${CURDIR}/../../target/riscv32imac-unknown-xous-elf/release
${WORKDIR}                    ${CURDIR}/../../target/ipc-ci
${FLASH_FILE}                 ${WORKDIR}/flash-robot.bin
${CONSOLE_LOG}                ${WORKDIR}/console-robot.log
${KERNEL_LOG}                 ${WORKDIR}/kernel-robot.log
# Virtual-time seconds. PRE-CALIBRATION PLACEHOLDERS.
${BOOT_TIMEOUT}               120
${TESTS_TIMEOUT}              900
# A failed run's emulation snapshot (128 MiB file-backed flash) is useless
# for triage; the console/kernel/renode logs suffice.
${CREATE_SNAPSHOT_ON_FAIL}    False

*** Keywords ***
Prepare Fresh Flash
    [Documentation]           Blank 0xFF NOR image; the flash model writes
    ...                       through into this file, so it is per-run. The
    ...                       image has no persistent state, but the flash
    ...                       model still requires a backing file.
    Create Directory          ${WORKDIR}
    Remove File               ${FLASH_FILE}
    Remove File               ${CONSOLE_LOG}
    Remove File               ${KERNEL_LOG}
    Evaluate                  open(r'${FLASH_FILE}', 'wb').write(b'\\xff' * 134217728)

*** Test Cases ***
Boot And Run IPC Tests
    Prepare Fresh Flash
    Execute Command           $image_dir=@${IMAGE_DIR}
    Execute Command           $flash_file=@${FLASH_FILE}
    Execute Command           $console_log=@${CONSOLE_LOG}
    Execute Command           $kernel_log=@${KERNEL_LOG}
    Execute Command           include @${CURDIR}/ipc-ci.resc
    Create Terminal Tester    sysbus.console    machine=SoC    timeout=${BOOT_TIMEOUT}
    # Fail-fast on any panic banner: the ipc-tests runner installs a panic
    # hook so CAUGHT test panics never print this -- any occurrence is a real
    # service death, after which the console goes silent.
    Register Failing Uart String    PANIC in PID

    Wait For Line On Uart     Welcome to Xous    timeout=${BOOT_TIMEOUT}

    # Sentinel stream (grammar: services/ipc-tests/src/main.rs). Pass
    # criteria: DONE present with fail=0 and xpass=0, final 'CI done', and no
    # stray FAIL/panic lines in the console log. The DONE counters are
    # asserted from the console log file rather than the tester result
    # object (portable across robotframework/remote-protocol versions).
    Wait For Line On Uart     IPC-TESTS DONE:    timeout=${TESTS_TIMEOUT}
    Wait For Line On Uart     CI done    timeout=60

    # The console file backend flushes every write, so the log is current.
    ${log}=                   Get File    ${CONSOLE_LOG}    encoding_errors=replace
    Should Match Regexp       ${log}    IPC-TESTS DONE: pass=\\d+ fail=0 xfail=\\d+ xpass=0 total=\\d+
    Should Not Match Regexp   ${log}    TEST \\S+ FAIL
    Should Not Contain        ${log}    PANIC in PID

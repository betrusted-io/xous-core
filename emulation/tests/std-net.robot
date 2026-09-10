*** Comments ***
std::net suite for `renode-test` (the CI path).

STATUS: adapted from emulation/tests/pddb-fs.robot alongside the fs suite's
harness; not yet exercised end to end under renode-test. Timeouts below are
PRE-CALIBRATION PLACEHOLDERS (see *** Variables ***) -- tighten once green-run
timings exist. tools/std-net-ci.py is the reference driver; this suite mirrors
its milestone sequence and the two must be kept in lockstep.

Unlike the fs suite there is no first-boot UX to drive: the net image has no
PDDB, no keyboard, no graphics service, so boot is fully unattended from the
loader through 'CI done'. Nothing here corresponds to the fs suite's
REQFMT/PIN/EC-abort choreography or its Inject Keys keywords.

Known delta vs. the python driver (accepted for the linear robot form):
- the python driver's 180 s host INACTIVITY reaper cannot be expressed in
  robot's linear form; instead 'PANIC in PID' is a registered failing UART
  string (fails the pending wait immediately -- a dead net service prints
  exactly that banner before going silent), and the overall wait is
  virtual-time bounded.
- robot timeouts are VIRTUAL-time seconds (decoupled from host speed).


*** Settings ***
Documentation                 Boot betrusted-soc+ec under Renode, wait for the
...                           net service to report an interface address,
...                           then assert on the net-tests sentinel stream.
Suite Setup                   Setup
Suite Teardown                Teardown
Test Teardown                 Test Teardown
Resource                      ${RENODEKEYWORDS}
Library                       OperatingSystem

*** Variables ***
${IMAGE_DIR}                  ${CURDIR}/../../target/riscv32imac-unknown-xous-elf/release
${WORKDIR}                    ${CURDIR}/../../target/std-net-ci
${FLASH_FILE}                 ${WORKDIR}/flash-robot.bin
${CONSOLE_LOG}                ${WORKDIR}/console-robot.log
${KERNEL_LOG}                 ${WORKDIR}/kernel-robot.log
# Virtual-time seconds (host-speed independent). PRE-CALIBRATION PLACEHOLDERS:
# no green run has produced real timings yet (see *** Comments ***).
${BOOT_TIMEOUT}               120
${NET_READY_TIMEOUT}          300
${TESTS_TIMEOUT}              1800
# A failed run's emulation snapshot (2 machines + 128 MiB file-backed flash)
# is huge and useless for triage; the console/kernel/renode logs suffice.
# Overrides the renode-keywords.robot default (suite variables win).
${CREATE_SNAPSHOT_ON_FAIL}    False

*** Keywords ***
Prepare Fresh Flash
    [Documentation]           Blank 0xFF NOR image; the flash model writes
    ...                       through into this file, so it is per-run. The
    ...                       net image has no persistent state, but the
    ...                       flash model still requires a backing file.
    Create Directory          ${WORKDIR}
    Remove File               ${FLASH_FILE}
    Remove File               ${CONSOLE_LOG}
    Remove File               ${KERNEL_LOG}
    Evaluate                  open(r'${FLASH_FILE}', 'wb').write(b'\\xff' * 134217728)

*** Test Cases ***
Boot And Run Net Tests
    Prepare Fresh Flash
    Execute Command           $image_dir=@${IMAGE_DIR}
    Execute Command           $flash_file=@${FLASH_FILE}
    Execute Command           $console_log=@${CONSOLE_LOG}
    Execute Command           $kernel_log=@${KERNEL_LOG}
    Execute Command           include @${CURDIR}/net-ci.resc
    # net-ci.resc ends with 'start'; the first marker is well after reset, so
    # creating the tester right after the include is safe.
    Create Terminal Tester    sysbus.console    machine=SoC    timeout=${BOOT_TIMEOUT}
    # Fail-fast on any panic banner: the net-tests runner installs a panic
    # hook so CAUGHT test panics never print this -- any occurrence is a real
    # service death, after which the console goes silent and every later
    # wait would time out.
    Register Failing Uart String    PANIC in PID

    Wait For Line On Uart     Welcome to Xous    timeout=${BOOT_TIMEOUT}
    Wait For Line On Uart     _|TT|_NET.OK,    timeout=${NET_READY_TIMEOUT}

    # Sentinel stream (grammar: services/net-tests/src/main.rs). Pass
    # criteria: DONE present with fail=0 and xpass=0, final 'CI done', and no
    # stray FAIL/panic lines in the console log. The DONE counters are
    # asserted from the console log file rather than the tester result
    # object (portable across robotframework/remote-protocol versions).
    Wait For Line On Uart     NET-TESTS DONE:    timeout=${TESTS_TIMEOUT}
    Wait For Line On Uart     CI done    timeout=60

    # The console file backend flushes every write, so the log is current.
    ${log}=                   Get File    ${CONSOLE_LOG}    encoding_errors=replace
    Should Match Regexp       ${log}    NET-TESTS DONE: pass=\\d+ fail=0 xfail=\\d+ xpass=0 total=\\d+
    Should Not Match Regexp   ${log}    TEST \\S+ FAIL
    Should Not Contain        ${log}    PANIC in PID

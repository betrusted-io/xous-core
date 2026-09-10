*** Comments ***
PDDB std::fs suite for `renode-test` (the CI path).

STATUS: written alongside the harness, validated against renode-test
1.16.1.4499 (robotframework 7.4.2; renode-test warns it wants 6.1 but the
suite passes on both). tools/pddb-fs-ci.py is the reference driver; this
suite mirrors its first-boot choreography and the two must be kept in
lockstep.

Known deltas vs. the python driver (accepted for the linear robot form):
- the rare 'EC update aborted' notification (EC boot race) and the
  PDDB.PWFAIL retry loop are not handled; such a run fails and is treated as
  a flake — the python driver dismisses/retries them.
- no warm-boot pass and no offline pddbdbg.py audit: the driver's cold+warm
  sequence stays the deep local gate; CI runs the cold leg only. (Warm-boot
  persistence is still exercised INSIDE the suite by the persist theme's
  pre/post state keys; a full remount leg can be added later as a second
  test case reusing ${FLASH_FILE}.)
- the python driver's 180 s host INACTIVITY reaper cannot be expressed in
  robot's linear form; instead 'PANIC in PID' is a registered failing UART
  string (fails the pending wait immediately — a dead pddb server prints
  exactly that banner before going silent), and the overall waits are
  virtual-time bounded.
- robot timeouts are VIRTUAL-time seconds (decoupled from host speed);
  Sleep is host time, used only for the marker-to-focus grace period.


*** Settings ***
Documentation                 Boot betrusted-soc under Renode, format the PDDB
...                           through the first-boot UX, then assert on the
...                           pddb-fs-tests sentinel stream.
Suite Setup                   Setup
Suite Teardown                Teardown
Test Teardown                 Test Teardown
Resource                      ${RENODEKEYWORDS}
Library                       OperatingSystem

*** Variables ***
${IMAGE_DIR}                  ${CURDIR}/../../target/riscv32imac-unknown-xous-elf/release
${WORKDIR}                    ${CURDIR}/../../target/pddb-fs-ci
${FLASH_FILE}                 ${WORKDIR}/flash-robot.bin
${CONSOLE_LOG}                ${WORKDIR}/console-robot.log
${KERNEL_LOG}                 ${WORKDIR}/kernel-robot.log
# Virtual-time seconds, calibrated from the 110-test-suite measurements
# (virtual ~= host / 4.4 on the reference box; virtual-time budgets are
# host-speed-independent):
# - launch -> 'status: starting main loop': ~9 virtual s  (BOOT_TIMEOUT 120)
# - REQFMT Okay -> mounted (4 MiB smalldb format): ~14 virtual s
#   (FORMAT_TIMEOUT 600)
# - MOUNTED -> FS-TESTS DONE, 112 tests: ~180 virtual s measured cold
#   (TESTS_TIMEOUT 3600 => ~20x margin; the python driver additionally
#   applies a 180 s host INACTIVITY reaper, which robot's linear form cannot
#   express — the registered 'PANIC in PID' failing string is the fail-fast
#   substitute for the dominant hang mode, a dead pddb server).
${BOOT_TIMEOUT}               120
${FORMAT_TIMEOUT}             600
${TESTS_TIMEOUT}              3600
# Markers are logged just BEFORE the modal gains focus; grace period (host
# time) before injecting.
${FOCUS_DELAY}                3s
# A failed run's emulation snapshot (2 machines + 128 MiB file-backed flash)
# is huge and useless for triage; the console/kernel/renode logs suffice.
# Overrides the renode-keywords.robot default (suite variables win).
${CREATE_SNAPSHOT_ON_FAIL}    False

*** Keywords ***
Prepare Fresh Flash
    [Documentation]           Blank 0xFF NOR image; the flash model writes
    ...                       through into this file, so it is per-run.
    Create Directory          ${WORKDIR}
    Remove File               ${FLASH_FILE}
    Remove File               ${CONSOLE_LOG}
    Remove File               ${KERNEL_LOG}
    Evaluate                  open(r'${FLASH_FILE}', 'wb').write(b'\\xff' * 134217728)

Inject Keys
    [Documentation]           The keyboard lives on the SoC machine; select it
    ...                       before every injection batch.
    [Arguments]               @{commands}
    Execute Command           mach set "SoC"
    FOR    ${command}    IN    @{commands}
        Execute Command       ${command}
        Sleep                 0.3s
    END

Inject Keys With Echo
    [Documentation]           Inject after the marker-to-focus grace period;
    ...                       verify delivery via the keyboard service's
    ...                       per-char 'injecting key' echo, with one
    ...                       re-injection retry.
    [Arguments]               @{commands}
    Sleep                     ${FOCUS_DELAY}
    Inject Keys               @{commands}
    ${ok}=                    Run Keyword And Return Status
    ...                       Wait For Line On Uart    injecting key    timeout=10
    IF    not ${ok}
        Log                   no injection echo; re-injecting once    WARN
        Inject Keys           @{commands}
        Wait For Line On Uart    injecting key    timeout=10
    END

Inject Arrow Once
    [Arguments]               ${letter}    ${code}
    Inject Keys               sysbus.keyboard WriteDoubleWord 0x0 0x1b
    ...                       sysbus.keyboard WriteDoubleWord 0x0 0x5b
    ...                       sysbus.keyboard WriteDoubleWord 0x0 ${letter}
    Wait For Line On Uart     injecting key .*\\(${code}\\)    timeout=10    treatAsRegex=true

Inject Arrow
    [Documentation]           Queue ESC [ <letter> in the keyboard model
    ...                       (UART_CHAR register writes) and wait for the
    ...                       keyboard service's echo of the parsed arrow,
    ...                       with one retry.
    [Arguments]               ${letter}    ${code}
    ${ok}=                    Run Keyword And Return Status
    ...                       Inject Arrow Once    ${letter}    ${code}
    IF    not ${ok}
        Log                   no arrow echo; re-injecting once    WARN
        Inject Arrow Once     ${letter}    ${code}
    END

Answer Format Prompt With Okay
    [Documentation]           Radio [Okay, Cancel], cursor on the item row,
    ...                       Okay preselected: Down x2 to the OK row, CR.
    Sleep                     ${FOCUS_DELAY}
    Inject Arrow              0x42    2193
    Inject Arrow              0x42    2193
    Inject Keys With Echo     sysbus.keyboard InjectLine ""

*** Test Cases ***
Boot Format And Run PDDB FS Tests
    Prepare Fresh Flash
    Execute Command           $image_dir=@${IMAGE_DIR}
    Execute Command           $flash_file=@${FLASH_FILE}
    Execute Command           $console_log=@${CONSOLE_LOG}
    Execute Command           $kernel_log=@${KERNEL_LOG}
    Execute Command           include @${CURDIR}/pddb-ci.resc
    # pddb-ci.resc ends with 'start'; the first marker is >5 virtual seconds
    # after reset, so creating the tester right after the include is safe.
    Create Terminal Tester    sysbus.console    machine=SoC    timeout=${BOOT_TIMEOUT}
    # Fail-fast on any panic banner: the pddb-fs-tests runner installs a
    # panic hook so CAUGHT test panics never print this — any occurrence is
    # a real server/service death (e.g. the PFC-1 truncate panic), after
    # which the console goes silent and every later wait would time out.
    Register Failing Uart String    PANIC in PID

    Wait For Line On Uart     status: starting main loop    timeout=${BOOT_TIMEOUT}
    Wait For Line On Uart     Requesting login password    timeout=${BOOT_TIMEOUT}

    # Format prompt: the arrows go through the keyboard model's inject queue
    # as ESC [ B, which the keyboard service echoes per key. Scan-matrix
    # Press/Release leaves no echo and is lost when the release lands before
    # the interrupt handler has read the matrix.
    Wait For Line On Uart     _|TT|_PDDB.REQFMT,_|TE|_    timeout=${BOOT_TIMEOUT}
    Answer Format Prompt With Okay

    # PIN entry #1
    Wait For Line On Uart     _|TT|_ROOTKEY.BOOTPW,_|TE|_    timeout=${BOOT_TIMEOUT}
    Inject Keys With Echo     sysbus.keyboard InjectLine "a"

    # press-any-key notification
    Wait For Line On Uart     _|TT|_PDDB.CHECKPASS,_|TE|_    timeout=${BOOT_TIMEOUT}
    Inject Keys With Echo     sysbus.keyboard InjectLine ""

    # PIN entry #2 (confirm)
    Wait For Line On Uart     _|TT|_ROOTKEY.BOOTPW,_|TE|_    timeout=${BOOT_TIMEOUT}
    Inject Keys With Echo     sysbus.keyboard InjectLine "a"

    # Unattended format, then mount
    Wait For Line On Uart     Erasing the PDDB region    timeout=${BOOT_TIMEOUT}
    Wait For Line On Uart     PDDB mount operation finished successfully    timeout=${FORMAT_TIMEOUT}
    Wait For Line On Uart     _|TT|_PDDB.MOUNTED,_|TE|_    timeout=${FORMAT_TIMEOUT}

    # Sentinel stream (grammar: services/pddb-fs-tests/README.md).
    # Pass criteria: DONE present with fail=0 and xpass=0, final 'CI done',
    # and no stray FAIL/panic lines in the console log. The DONE counters are
    # asserted from the console log file rather than the tester result object
    # (portable across robotframework/remote-protocol versions).
    Wait For Line On Uart     FS-TESTS DONE:    timeout=${TESTS_TIMEOUT}
    Wait For Line On Uart     CI done    timeout=60

    # The console file backend flushes every write, so the log is current.
    ${log}=                   Get File    ${CONSOLE_LOG}    encoding_errors=replace
    Should Match Regexp       ${log}    FS-TESTS DONE: pass=\\d+ fail=0 xfail=\\d+ xpass=0 total=\\d+
    Should Not Match Regexp   ${log}    TEST \\S+ FAIL
    Should Not Contain        ${log}    PANIC in PID

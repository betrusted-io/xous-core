#! /usr/bin/env python3
"""Fast local loop driver for the PDDB std::fs suite under Renode.

Per run: (re)creates a blank 0xFF flash backing file, launches a headless
Renode (emulation/tests/pddb-ci.resc, SoC + EC), drives the boot UX through
the monitor (stdin FIFO) as a marker state machine tailing the console UART
log, parses the test sentinel stream, then audits the resulting flash image
offline with tools/pddbdbg.py.

Choreography, markers, pass criteria, and the sentinel grammar are documented
in services/pddb-fs-tests/README.md.

Build the image first: cargo xtask pddb-fs-ci
Typical use: python3 tools/pddb-fs-ci.py            (one cold run + audit)
             python3 tools/pddb-fs-ci.py --runs 20  (flake hunting)
"""
import argparse
import logging
import os
import re
import shutil
import subprocess
import sys
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

FLASH_SIZE = 134217728  # 128 MiB MX66UM1G45G NOR
FLASH_CHUNK = 1024 * 1024

# Console (log-server UART) markers -- see services/pddb-fs-tests/README.md.
MARK_STATUS_MAIN = 'status: starting main loop'
MARK_PW_REQUEST = 'Requesting login password'
MARK_REQFMT = '_|TT|_PDDB.REQFMT,_|TE|_'
MARK_BOOTPW = '_|TT|_ROOTKEY.BOOTPW,_|TE|_'
MARK_CHECKPASS = '_|TT|_PDDB.CHECKPASS,_|TE|_'
MARK_PWFAIL = '_|TT|_PDDB.PWFAIL,_|TE|_'
MARK_BADPW = '_|TT|_PDDB.BADPW,_|TE|_'
MARK_MOUNTED = '_|TT|_PDDB.MOUNTED,_|TE|_'
MARK_ERASE = 'Erasing the PDDB region'
MARK_ERASE_PROGRESS = "Cryptographic 'erase'"
MARK_MOUNT_OK = 'PDDB mount operation finished successfully'
MARK_ATTEMPT_MOUNT = 'Attempting to mount the PDDB'
MARK_EC_ABORT = 'EC update aborted'
MARK_INJECT_ECHO = 'injecting key'  # keyboard service echo, per injected char
MARK_PANIC = 'PANIC in PID'

RE_TEST = re.compile(r'TEST (\S+) (PASS|FAIL|XFAIL|XPASS)(?:\s+(.*))?$')
RE_DONE = re.compile(
    r'FS-TESTS DONE: pass=(\d+) fail=(\d+) xfail=(\d+) xpass=(\d+) total=(\d+)')

# Markers are logged just BEFORE the modal gains focus; give the GAM time to
# hand focus over before injecting.
FOCUS_DELAY = 1.5
# Wait this long for the keyboard service's per-char injection echo before
# re-injecting (one retry).
ECHO_TIMEOUT = 5.0
MAX_PIN_ATTEMPTS = 3


class RunFailure(Exception):
    pass


class ConsoleTail:
    """Incremental tail of the console UART CreateFileBackend log."""

    def __init__(self, path):
        self.path = path
        self.f = None
        self.partial = b''
        self.echo_count = 0
        self.panic_lines = []

    def close(self):
        if self.f is not None:
            self.f.close()
            self.f = None

    def poll(self):
        """Return a list of complete new lines (str) since the last poll."""
        if self.f is None:
            if not os.path.exists(self.path):
                return []
            self.f = open(self.path, 'rb')
        data = self.f.read()
        if not data:
            return []
        self.partial += data
        raw_lines = self.partial.split(b'\n')
        self.partial = raw_lines.pop()
        lines = []
        for raw in raw_lines:
            line = raw.decode('utf-8', errors='replace').rstrip('\r')
            self.echo_count += line.count(MARK_INJECT_ECHO)
            if MARK_PANIC in line:
                self.panic_lines.append(line)
            lines.append(line)
        return lines


class RenodeRun:
    """One Renode launch: monitor FIFO, console tail, boot state machine."""

    def __init__(self, args, run_idx, first_boot):
        self.args = args
        self.run_idx = run_idx
        self.first_boot = first_boot
        self.workdir = args.workdir
        self.flash_file = os.path.join(args.workdir, 'flash.bin')
        self.console_log = os.path.join(args.workdir, 'console.log')
        self.kernel_log = os.path.join(args.workdir, 'kernel.log')
        self.run_log = os.path.join(args.workdir, 'run.log')
        self.fifo_path = os.path.join(args.workdir, 'monitor.fifo')
        self.proc = None
        self.mon_fd = None
        self.run_log_f = None
        self.tail = ConsoleTail(self.console_log)
        # Console lines polled but not yet consumed by a wait. Without this
        # buffer, a wait that matches mid-batch would silently drop the rest
        # of the batch -- and consecutive markers (e.g. 'Requesting login
        # password' then PDDB.REQFMT) routinely land in the same poll.
        self.pending = []
        self.t0 = None
        self.last_progress = 0.0

    # ---------- infrastructure ----------

    def milestone(self, msg):
        elapsed = 0.0 if self.t0 is None else time.time() - self.t0
        logging.info('[%8.1fs] %s', elapsed, msg)

    def launch(self):
        for stale in (self.console_log, self.kernel_log, self.run_log):
            if os.path.exists(stale):
                os.remove(stale)
        if os.path.exists(self.fifo_path):
            os.remove(self.fifo_path)
        os.mkfifo(self.fifo_path)
        # O_RDWR on a FIFO opens without blocking and keeps a writer alive, so
        # Renode's stdin never sees EOF; we write injections to the same fd.
        self.mon_fd = os.open(self.fifo_path, os.O_RDWR)
        self.run_log_f = open(self.run_log, 'wb')
        monitor_setup = '; '.join([
            '$image_dir=@{}'.format(self.args.image_dir),
            '$flash_file=@{}'.format(self.flash_file),
            '$console_log=@{}'.format(self.console_log),
            '$kernel_log=@{}'.format(self.kernel_log),
            'i @{}'.format(self.args.resc),
        ])
        cmd = [self.args.renode, '--disable-gui', '--console',
               '-e', monitor_setup]
        self.t0 = time.time()
        self.proc = subprocess.Popen(
            cmd,
            stdin=self.mon_fd,
            stdout=self.run_log_f,
            stderr=subprocess.STDOUT,
            cwd=REPO_ROOT,
        )
        if self.args.pid_file:
            with open(self.args.pid_file, 'w') as f:
                f.write(str(self.proc.pid))
        self.milestone('renode launched (pid {})'.format(self.proc.pid))

    def monitor(self, command):
        """Write one monitor command line to the FIFO."""
        logging.debug('monitor <- %s', command)
        os.write(self.mon_fd, (command + '\n').encode('utf-8'))

    def check_alive(self):
        if self.proc.poll() is not None:
            raise RunFailure(
                'renode exited prematurely (code {}); see {}'.format(
                    self.proc.returncode, self.run_log))

    def shutdown(self):
        """Quit cleanly (flushes the flash BackingFile), then escalate."""
        try:
            if self.proc is not None and self.proc.poll() is None:
                try:
                    self.monitor('quit')
                except OSError:
                    pass
                try:
                    self.proc.wait(timeout=30)
                    self.milestone('renode quit cleanly')
                except subprocess.TimeoutExpired:
                    logging.warning('renode did not quit within 30 s; killing')
                    self.proc.kill()
                    self.proc.wait()
        finally:
            # Belt and braces: never leave a renode behind.
            if self.proc is not None and self.proc.poll() is None:
                try:
                    self.proc.kill()
                    self.proc.wait()
                except OSError:
                    pass
            if self.mon_fd is not None:
                os.close(self.mon_fd)
                self.mon_fd = None
            if self.run_log_f is not None:
                self.run_log_f.close()
                self.run_log_f = None
            self.tail.close()
            if os.path.exists(self.fifo_path):
                os.remove(self.fifo_path)
            if self.args.pid_file and os.path.exists(self.args.pid_file):
                os.remove(self.args.pid_file)

    # ---------- console state machine primitives ----------

    def poll_console(self):
        """Poll the console tail into the pending-line buffer (logging each
        new line once). All console consumption goes through self.pending so
        a wait that stops mid-batch never drops the lines behind it."""
        new_lines = self.tail.poll()
        for line in new_lines:
            logging.debug('console: %s', line)
        self.pending.extend(new_lines)

    def wait_for(self, markers, timeout, ec_abort_ok=False):
        """Consume console lines until one of `markers` appears.

        Returns the matched marker. If ec_abort_ok, dismisses the rare
        'EC update aborted' notification (EC boot race) and keeps waiting.
        """
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.check_alive()
            self.poll_console()
            while self.pending:
                line = self.pending.pop(0)
                if ec_abort_ok and MARK_EC_ABORT in line:
                    self.milestone(
                        "'EC update aborted' notification -> dismissing")
                    time.sleep(FOCUS_DELAY)
                    self.inject(['sysbus.keyboard InjectLine ""'])
                    continue
                if MARK_ERASE_PROGRESS in line \
                        and time.time() - self.last_progress > 5.0:
                    # Liveness signal during the unattended format
                    # (throttled: one line per ~5 s).
                    self.last_progress = time.time()
                    self.milestone('format progress: {}'.format(line.strip()))
                matched = None
                for marker in markers:
                    if marker in line:
                        matched = marker
                        break
                if matched is not None:
                    self.milestone('marker: {}'.format(matched))
                    return matched
            time.sleep(0.2)
        raise RunFailure('timeout ({} s) waiting for {}'.format(
            timeout, ' | '.join(markers)))

    def inject(self, keyboard_commands, delay=0.3):
        """Send keyboard commands (keyboard lives on the SoC machine)."""
        self.monitor('mach set "SoC"')
        time.sleep(delay)
        for command in keyboard_commands:
            self.monitor(command)
            time.sleep(delay)

    def inject_verified(self, keyboard_commands, what):
        """Inject after the marker->focus delay; verify via the keyboard
        service's 'injecting key' echo, with one re-injection retry."""
        time.sleep(FOCUS_DELAY)
        for attempt in (1, 2):
            baseline = self.tail.echo_count
            self.inject(keyboard_commands)
            deadline = time.time() + ECHO_TIMEOUT
            while time.time() < deadline:
                self.check_alive()
                # poll_console preserves the lines for the next wait_for;
                # echo_count is a tail-side counter independent of consumption
                self.poll_console()
                if self.tail.echo_count > baseline:
                    self.milestone('injected: {}'.format(what))
                    return
                time.sleep(0.2)
            if attempt == 1:
                logging.warning(
                    'no injection echo within %.0f s for %s; re-injecting',
                    ECHO_TIMEOUT, what)
        # Echo lost twice: continue and let the next marker wait decide.
        logging.warning('no injection echo after retry for %s; continuing',
                        what)

    # ---------- boot choreography ----------

    def drive_boot(self):
        if self.first_boot:
            self.drive_first_boot()
        else:
            self.drive_warm_boot()

    def drive_first_boot(self):
        t_boot = self.args.timeout_boot
        self.wait_for([MARK_STATUS_MAIN], t_boot, ec_abort_ok=True)
        self.wait_for([MARK_PW_REQUEST], t_boot, ec_abort_ok=True)
        # Radio [Okay, Cancel], cursor on the item row: Down x2 to reach the
        # OK row (CR on the item row only sets the payload), then CR.
        self.wait_for([MARK_REQFMT], t_boot, ec_abort_ok=True)
        self.inject_verified([
            'sysbus.keyboard Press Down',
            'sysbus.keyboard Release Down',
            'sysbus.keyboard Press Down',
            'sysbus.keyboard Release Down',
            'sysbus.keyboard InjectLine ""',
        ], 'REQFMT Okay')
        for attempt in range(1, MAX_PIN_ATTEMPTS + 1):
            # PIN entry #1
            self.wait_for([MARK_BOOTPW], t_boot)
            self.inject_verified(['sysbus.keyboard InjectLine "a"'],
                                 'boot PIN')
            # press-any-key notification
            self.wait_for([MARK_CHECKPASS], t_boot)
            self.inject_verified(['sysbus.keyboard InjectLine ""'],
                                 'CHECKPASS dismiss')
            # PIN entry #2 (confirm)
            self.wait_for([MARK_BOOTPW], t_boot)
            self.inject_verified(['sysbus.keyboard InjectLine "a"'],
                                 'boot PIN confirm')
            marker = self.wait_for([MARK_ERASE, MARK_PWFAIL], t_boot)
            if marker == MARK_ERASE:
                break
            logging.warning('PIN mismatch (PDDB.PWFAIL), attempt %d', attempt)
        else:
            raise RunFailure('PIN never accepted after {} attempts'.format(
                MAX_PIN_ATTEMPTS))
        # Unattended format, then mount.
        self.wait_for([MARK_MOUNT_OK], self.args.timeout_format)
        self.wait_for([MARK_MOUNTED], self.args.timeout_format)

    def drive_warm_boot(self):
        t_boot = self.args.timeout_boot
        self.wait_for([MARK_STATUS_MAIN], t_boot, ec_abort_ok=True)
        self.wait_for([MARK_PW_REQUEST], t_boot, ec_abort_ok=True)
        for attempt in range(1, MAX_PIN_ATTEMPTS + 1):
            self.wait_for([MARK_BOOTPW], t_boot, ec_abort_ok=True)
            self.inject_verified(['sysbus.keyboard InjectLine "a"'],
                                 'boot PIN')
            marker = self.wait_for(
                [MARK_MOUNTED, MARK_ATTEMPT_MOUNT, MARK_BADPW],
                self.args.timeout_format)
            if marker == MARK_ATTEMPT_MOUNT:
                marker = self.wait_for([MARK_MOUNTED, MARK_BADPW],
                                       self.args.timeout_format)
            if marker == MARK_MOUNTED:
                return
            # PDDB.BADPW: retry radio [Yes/No], Yes preselected -> CR.
            logging.warning('bad PIN (PDDB.BADPW), attempt %d', attempt)
            time.sleep(FOCUS_DELAY)
            self.inject(['sysbus.keyboard InjectLine ""'])
        raise RunFailure('PDDB never mounted after {} PIN attempts'.format(
            MAX_PIN_ATTEMPTS))

    # ---------- sentinel stream ----------

    def collect_results(self):
        """After PDDB.MOUNTED: parse TEST/DONE sentinels; return failures.

        Two timeouts govern the sentinel stream:
        - INACTIVITY (--timeout-inactivity, default 180 s): no new console
          output at all for that long. A dead pddb server (e.g. the PFC-1
          truncate panic) manifests exactly this way -- every later fs call
          blocks forever, so the console just goes quiet. On expiry the run
          fails, reporting the last TEST sentinel seen (the killer is the
          test AFTER it, or that test itself if it never completed) and the
          trailing console lines.
        - OVERALL (--timeout-tests, default 5400 s): a generous whole-suite
          cap so a pathological-but-chatty stream still terminates.
        """
        problems = []
        counts = {'PASS': 0, 'FAIL': 0, 'XFAIL': 0, 'XPASS': 0}
        done = None
        last_sentinel = None
        trailing = []  # last console lines, for the inactivity report
        last_activity = time.time()
        deadline = time.time() + self.args.timeout_tests
        while done is None:
            now = time.time()
            if now >= deadline:
                problems.append(
                    'overall test cap hit: no FS-TESTS DONE within {} s '
                    '(last sentinel: {})'.format(
                        self.args.timeout_tests, last_sentinel or 'none'))
                break
            if now - last_activity >= self.args.timeout_inactivity:
                problems.append(
                    'console INACTIVE for {} s -- pddb server presumed dead. '
                    'Last TEST sentinel: {}. Trailing console lines:\n  {}'
                    .format(self.args.timeout_inactivity,
                            last_sentinel or 'none (no test completed)',
                            '\n  '.join(trailing[-12:]) or '(none)'))
                break
            self.check_alive()
            before = len(self.pending)
            self.poll_console()
            if len(self.pending) > before:
                last_activity = time.time()
                trailing.extend(self.pending[before:])
                del trailing[:-20]
            while self.pending and done is None:
                line = self.pending.pop(0)
                m = RE_TEST.search(line)
                if m:
                    name, status, detail = m.group(1), m.group(2), m.group(3)
                    counts[status] += 1
                    last_sentinel = 'TEST {} {}'.format(name, status)
                    self.milestone('TEST {} {}{}'.format(
                        name, status, ' ' + detail if detail else ''))
                    if status == 'FAIL':
                        problems.append('test failed: {} ({})'.format(
                            name, detail))
                    elif status == 'XPASS':
                        problems.append(
                            'unexpected pass: {} ({}) -- update the XFAIL '
                            'registry'.format(name, detail))
                    continue
                m = RE_DONE.search(line)
                if m:
                    done = tuple(int(x) for x in m.groups())
                    self.milestone('FS-TESTS DONE: pass={} fail={} xfail={} '
                                   'xpass={} total={}'.format(*done))
                    break
            time.sleep(0.2)
        if done is not None:
            _, n_fail, _, n_xpass, n_total = done
            if n_fail != 0:
                problems.append('DONE reports fail={}'.format(n_fail))
            if n_xpass != 0:
                problems.append('DONE reports xpass={}'.format(n_xpass))
            if sum(counts.values()) != n_total:
                logging.warning(
                    'sentinel count mismatch: saw %d TEST lines, DONE says '
                    'total=%d', sum(counts.values()), n_total)
        # Drain any trailing output (panic banners race the DONE line).
        settle = time.time() + 5
        while time.time() < settle:
            self.poll_console()
            self.pending.clear()
            if self.proc.poll() is not None:
                break
            time.sleep(0.5)
        for line in self.tail.panic_lines:
            problems.append('stray panic: {}'.format(line.strip()))
        return problems


def write_blank_flash(path):
    """Fresh blank-NOR image: 0xFF-filled, written in 1 MiB chunks."""
    chunk = b'\xff' * FLASH_CHUNK
    with open(path, 'wb') as f:
        for _ in range(FLASH_SIZE // FLASH_CHUNK):
            f.write(chunk)


def offline_audit(args):
    """Copy the flash image aside and audit it with pddbdbg.py.

    Deliberately NOT --ci: pddbdbg's CI mode requires every key name to carry
    the hosted-CI 'name|dict|...|len<n>' checksum structure and crashes
    (IndexError in pddbcommon.ci_check) on the system UserPrefsDict that every
    real boot creates -- empirically verified in the harness pilot. Instead
    require: exit code 0, zero ERROR/WARNING lines (dict-count mismatches log
    as ERROR either way), the .System basis decrypting, and at least one dict
    enumerating (UserPrefsDict always exists after first mount)."""
    problems = []
    flash_file = os.path.join(args.workdir, 'flash.bin')
    audit_image = os.path.join(REPO_ROOT, 'tools', 'pddb-images', 'renode.bin')
    shutil.copyfile(flash_file, audit_image)
    cmd = [sys.executable, os.path.join('tools', 'pddbdbg.py'),
           '--renode', '--smalldb', '--pin', 'a']
    logging.info('offline audit: %s', ' '.join(cmd))
    try:
        result = subprocess.run(
            cmd, cwd=REPO_ROOT, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, encoding='utf-8', errors='replace',
            timeout=300)
    except subprocess.TimeoutExpired:
        return ['offline audit timed out']
    basis_found = False
    dicts_found = 0
    for line in result.stdout.split('\n'):
        if 'ERROR' in line or 'WARNING' in line:
            logging.info(line.strip())
            problems.append('audit: {}'.format(line.strip()))
        if 'Basis .System' in line:
            basis_found = True
        if 'decrypt dict ' in line:
            dicts_found += 1
    if result.returncode != 0:
        problems.append('audit exited with code {}'.format(result.returncode))
    if not basis_found:
        problems.append('audit never decrypted the .System basis')
    if dicts_found == 0:
        problems.append('audit enumerated no dictionaries')
    if not problems:
        logging.info('offline audit OK: .System basis, %d dict(s)',
                     dicts_found)
    return problems


def preserve_failure_logs(args, run_idx):
    dest = os.path.join(args.workdir, 'failed-run-{}'.format(run_idx))
    os.makedirs(dest, exist_ok=True)
    for name in ('console.log', 'kernel.log', 'run.log'):
        src = os.path.join(args.workdir, name)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(dest, name))
    logging.info('failure logs preserved in %s', dest)


def do_run(args, run_idx):
    """One full run. Returns a list of problems (empty == PASS)."""
    os.makedirs(args.workdir, exist_ok=True)
    flash_file = os.path.join(args.workdir, 'flash.bin')
    first_boot = True
    if args.warm and os.path.exists(flash_file) \
            and os.path.getsize(flash_file) == FLASH_SIZE:
        first_boot = False
        logging.info('reusing existing flash file (warm boot): %s',
                     flash_file)
    else:
        if args.warm:
            logging.warning('--warm requested but no usable flash file; '
                            'starting cold')
        write_blank_flash(flash_file)
        logging.info('fresh 0xFF flash file written: %s', flash_file)

    run = RenodeRun(args, run_idx, first_boot)
    problems = []
    try:
        run.launch()
        run.drive_boot()
        problems.extend(run.collect_results())
    except RunFailure as e:
        problems.append(str(e))
    finally:
        # ALWAYS bring renode down, even on unexpected exceptions, so no
        # orphan eats CPU and the flash BackingFile gets flushed on quit.
        run.shutdown()

    if not problems and not args.skip_analysis:
        problems.extend(offline_audit(args))
    if problems:
        preserve_failure_logs(args, run_idx)
    return problems


def main():
    parser = argparse.ArgumentParser(
        description='Renode-based regression tester for PDDB std::fs')
    parser.add_argument(
        '--workdir', required=False, type=str,
        default=os.path.join(REPO_ROOT, 'target', 'pddb-fs-ci'),
        help='scratch directory for flash/logs/FIFO')
    parser.add_argument(
        '--resc', required=False, type=str,
        default=os.path.join(REPO_ROOT, 'emulation', 'tests', 'pddb-ci.resc'),
        help='machine-definition script to include')
    parser.add_argument(
        '--image-dir', required=False, type=str,
        default=os.path.join(REPO_ROOT, 'target',
                             'riscv32imac-unknown-xous-elf', 'release'),
        help='directory holding loader.bin and xous.img')
    parser.add_argument(
        '--renode', required=False, type=str, default='renode',
        help='renode executable')
    parser.add_argument(
        '--warm', required=False, action='store_true',
        help='reuse the existing flash file (warm-boot choreography)')
    parser.add_argument(
        '--runs', required=False, type=int, default=1,
        help='number of runs')
    parser.add_argument(
        '--skip-analysis', required=False, action='store_true',
        help='skip the offline pddbdbg.py audit stage')
    parser.add_argument(
        '--timeout-boot', required=False, type=int, default=240,
        help='seconds allowed for each pre-format boot marker')
    parser.add_argument(
        '--timeout-format', required=False, type=int, default=600,
        help='seconds allowed for PDDB format+mount')
    parser.add_argument(
        '--timeout-tests', required=False, type=int, default=5400,
        help='overall cap (seconds) for the whole test sentinel stream')
    parser.add_argument(
        '--timeout-inactivity', required=False, type=int, default=180,
        help='fail if the console emits NOTHING for this long after mount '
             '(a dead pddb server manifests as total console silence)')
    parser.add_argument(
        '--pid-file', required=False, type=str, default=None,
        help='write the renode pid here while it runs')
    parser.add_argument(
        '--loglevel', required=False, type=str, default='INFO',
        help='set logging level (INFO/DEBUG/WARNING/ERROR)')
    args = parser.parse_args()

    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    args.workdir = os.path.abspath(args.workdir)
    args.resc = os.path.abspath(args.resc)
    args.image_dir = os.path.abspath(args.image_dir)

    pass_log = {}
    for run_idx in range(int(args.runs)):
        logging.info('===== run %d/%d =====', run_idx + 1, args.runs)
        problems = do_run(args, run_idx)
        if problems:
            for p in problems:
                logging.error('run %d: %s', run_idx, p)
            pass_log[run_idx] = 'FAIL'
        else:
            pass_log[run_idx] = 'PASS'
        logging.info('run %d %s', run_idx, pass_log[run_idx])

    # summary report
    passing = True
    for items in pass_log.items():
        logging.info(items)
        if items[1] != 'PASS':
            passing = False
    if passing:
        logging.info('Overall pass, exiting with 0')
        exit(0)
    else:
        logging.info('A failure was detected, exiting with 1')
        exit(1)


if __name__ == '__main__':
    main()

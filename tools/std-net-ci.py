#! /usr/bin/env python3
"""Fast local loop driver for the std::net suite under Renode.

Per run: (re)creates a blank 0xFF flash backing file (the flash model needs a
backing file even though this image has no persistent state), launches a
headless Renode (emulation/tests/net-ci.resc, SoC + EC + wifi switch), tails
the console UART log for the boot and net-ready milestones, then parses the
net-tests sentinel stream. There is no first-boot UX to drive: the image has
no PDDB, no keyboard, no graphics service, so boot runs unattended straight
through to 'CI done'.

Sentinel grammar (`TEST <name> PASS|FAIL|XFAIL|XPASS`, then `NET-TESTS DONE:
pass=.. fail=.. xfail=.. xpass=.. total=..`, then `CI done`) and pass criteria
mirror services/pddb-fs-tests, implemented in services/net-tests.

Build the image first: cargo xtask std-net-ci
Typical use: python3 tools/std-net-ci.py            (one run)
             python3 tools/std-net-ci.py --runs 20   (flake hunting)
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

# Console (log-server UART) markers.
MARK_BOOT = 'Welcome to Xous'
MARK_NET_OK = '_|TT|_NET.OK,'
MARK_PANIC = 'PANIC in PID'

RE_TEST = re.compile(r'TEST (\S+) (PASS|FAIL|XFAIL|XPASS)(?:\s+(.*))?$')
RE_DONE = re.compile(
    r'NET-TESTS DONE: pass=(\d+) fail=(\d+) xfail=(\d+) xpass=(\d+) total=(\d+)')


class RunFailure(Exception):
    pass


class ConsoleTail:
    """Incremental tail of the console UART CreateFileBackend log."""

    def __init__(self, path):
        self.path = path
        self.f = None
        self.partial = b''
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
            if MARK_PANIC in line:
                self.panic_lines.append(line)
            lines.append(line)
        return lines


class RenodeRun:
    """One Renode launch: monitor FIFO, console tail, milestone waits."""

    def __init__(self, args, run_idx):
        self.args = args
        self.run_idx = run_idx
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
        # of the batch -- and consecutive markers routinely land in the same
        # poll (e.g. the boot banner and NET.OK on a fast host).
        self.pending = []
        self.t0 = None

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
        # Renode's stdin never sees EOF; monitor() writes to the same fd.
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

    def check_panic(self):
        """The net-tests runner installs a panic hook, so a caught test
        panic never prints this banner -- any occurrence is a real service
        death. Fail immediately rather than waiting out the inactivity or
        overall-cap timers."""
        if self.tail.panic_lines:
            raise RunFailure('PANIC in PID: {}'.format(
                self.tail.panic_lines[-1].strip()))

    def shutdown(self):
        """Quit cleanly, then escalate."""
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

    def wait_for(self, markers, timeout):
        """Consume console lines until one of `markers` appears; returns it."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.check_alive()
            self.poll_console()
            self.check_panic()
            while self.pending:
                line = self.pending.pop(0)
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

    # ---------- boot ----------

    def wait_for_ready(self):
        """No first-boot UX to drive here: unattended straight through to
        net-ready."""
        self.wait_for([MARK_BOOT], self.args.timeout_boot)
        self.wait_for([MARK_NET_OK], self.args.timeout_net_ready)

    # ---------- sentinel stream ----------

    def collect_results(self):
        """After NET.OK: parse TEST/DONE sentinels; return failures.

        Two timeouts govern the sentinel stream:
        - INACTIVITY (--timeout-inactivity, default 180 s): no new console
          output at all for that long. A wedged net service (e.g. a blocked
          socket call that never wakes) manifests exactly this way -- every
          later test blocks forever, so the console just goes quiet. On
          expiry the run fails, reporting the last TEST sentinel seen and the
          trailing console lines.
        - OVERALL (--timeout-tests, default 1800 s): a generous whole-suite
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
                    'overall test cap hit: no NET-TESTS DONE within {} s '
                    '(last sentinel: {})'.format(
                        self.args.timeout_tests, last_sentinel or 'none'))
                break
            if now - last_activity >= self.args.timeout_inactivity:
                problems.append(
                    'console INACTIVE for {} s -- net service presumed dead. '
                    'Last TEST sentinel: {}. Trailing console lines:\n  {}'
                    .format(self.args.timeout_inactivity,
                            last_sentinel or 'none (no test completed)',
                            '\n  '.join(trailing[-12:]) or '(none)'))
                break
            self.check_alive()
            before = len(self.pending)
            self.poll_console()
            self.check_panic()
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
                    self.milestone('NET-TESTS DONE: pass={} fail={} xfail={} '
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
    """Fresh blank-NOR image: 0xFF-filled, written in 1 MiB chunks. Required
    by the flash model even though this image never persists state."""
    chunk = b'\xff' * FLASH_CHUNK
    with open(path, 'wb') as f:
        for _ in range(FLASH_SIZE // FLASH_CHUNK):
            f.write(chunk)


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
    write_blank_flash(flash_file)
    logging.info('fresh 0xFF flash file written: %s', flash_file)

    run = RenodeRun(args, run_idx)
    problems = []
    try:
        run.launch()
        run.wait_for_ready()
        problems.extend(run.collect_results())
    except RunFailure as e:
        problems.append(str(e))
    finally:
        # ALWAYS bring renode down, even on unexpected exceptions, so no
        # orphan eats CPU.
        run.shutdown()

    if problems:
        preserve_failure_logs(args, run_idx)
    return problems


def main():
    parser = argparse.ArgumentParser(
        description='Renode-based regression tester for std::net')
    parser.add_argument(
        '--workdir', required=False, type=str,
        default=os.path.join(REPO_ROOT, 'target', 'std-net-ci'),
        help='scratch directory for flash/logs/FIFO')
    parser.add_argument(
        '--resc', required=False, type=str,
        default=os.path.join(REPO_ROOT, 'emulation', 'tests', 'net-ci.resc'),
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
        '--runs', required=False, type=int, default=1,
        help='number of runs')
    # Timeout defaults below are generous placeholders; tighten once real
    # run timings are known.
    parser.add_argument(
        '--timeout-boot', required=False, type=int, default=300,
        help='seconds allowed for the boot banner (wall clock)')
    parser.add_argument(
        '--timeout-net-ready', required=False, type=int, default=300,
        help='seconds allowed for NET.OK after the boot banner (wall clock)')
    parser.add_argument(
        '--timeout-tests', required=False, type=int, default=1800,
        help='overall cap (seconds) for the whole test sentinel stream')
    parser.add_argument(
        '--timeout-inactivity', required=False, type=int, default=180,
        help='fail if the console emits NOTHING for this long after net-ready '
             '(a wedged net service manifests as total console silence)')
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

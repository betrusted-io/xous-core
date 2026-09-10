#! /usr/bin/env python3
"""Local loop driver for the std::net Cross-host (cross-host) suite under Renode.

Cross-host boots the SoC + EC pair AND a real Linux peer (emulation/linux-server.resc)
on one Ethernet switch. Per run this driver:
  1. provisions a blank flash and a per-run scratch copy of the peer rootfs
     (the peer's CFI flash writes back into its backing file);
  2. launches headless Renode with emulation/tests/net-cross-host-ci.resc, which
     starts all three machines and exposes the peer's serial console on a pty;
  3. drives the peer console (press Enter for a passwordless root shell, then
     start the test services -- a `nc` TCP echo server) before the DUT boots
     far enough to run its tests;
  4. tails the SoC console UART log for the boot and net-ready milestones (the
     DUT takes a REAL DHCP lease from the peer's udhcpd -- the image is built
     with net/renode-peer, no static seed), then parses the net-tests sentinel
     stream exactly as tools/std-net-ci.py does.

The peer contract (ports, addresses, service shapes) MUST match
services/net-tests/src/tests/cross-host.rs -- keep the two in lockstep.

Build the image first: cargo xtask std-net-cross-host-ci
Typical use: python3 tools/std-net-cross-host-ci.py
"""
import argparse
import logging
import os
import re
import select
import shutil
import subprocess
import sys
import termios
import time
import tty

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

FLASH_SIZE = 134217728  # 128 MiB MX66UM1G45G NOR
FLASH_CHUNK = 1024 * 1024

PEER_ROOTFS_SRC = os.path.join(REPO_ROOT, 'emulation', 'linux-server-rootfs.jffs2')
PEER_BIN = os.path.join(REPO_ROOT, 'emulation', 'versatile-vmlinux')

# Peer service contract -- must match services/net-tests/src/tests/cross-host.rs.
PEER_IP = '192.168.0.1'
PEER_ECHO_TCP = 6001
PEER_ECHO_UDP = 6002
PEER_BULK_TCP = 6003
PEER_BULK_LEN = 8192  # bytes of 'A' the bulk source serves
# Static A records the peer's dnsd serves; the DUT's resolver (dns1, advertised
# by the peer's udhcpd) points at PEER_IP so std lookups reach this dnsd.
PEER_DNS_RECORDS = [
    ('peer.test', '192.168.0.1'),
    ('one.test', '10.11.12.13'),
    ('two.test', '203.0.113.7'),
]

# Console (log-server UART) markers on the SoC.
MARK_BOOT = 'Welcome to Xous'
MARK_NET_OK = '_|TT|_NET.OK,'
MARK_PANIC = 'PANIC in PID'

# Peer console markers.
PEER_BANNER = re.compile(rb'Please press Enter to activate this console')
PEER_PROMPT = re.compile(rb'# ')
PEER_MARK_RE = re.compile(rb'@@END@@:(\d+)')
PEER_MARK_CMD = ' ; echo "@@EN""D@@:$?"'

RE_TEST = re.compile(r'TEST (\S+) (PASS|FAIL|XFAIL|XPASS)(?:\s+(.*))?$')
RE_DONE = re.compile(
    r'NET-TESTS DONE: pass=(\d+) fail=(\d+) xfail=(\d+) xpass=(\d+) total=(\d+)')


class RunFailure(Exception):
    pass


class PeerSetupError(RunFailure):
    """A flake while provisioning the peer (pty/banner/console command). These
    are harness issues, not DUT results, so do_run retries the whole run once
    rather than counting a peer-console hiccup as a suite failure."""
    pass


class ConsoleTail:
    """Incremental tail of the SoC console UART CreateFileBackend log."""

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


class PeerConsole:
    """Hardened pty reader/writer for the Linux peer's serial console.

    The emulated 16550 occasionally drops the leading bytes of a written line,
    so mitigations are: chunked writes, an end-marker per command, an
    inactivity nudge (CR), and a ctrl-C + resend on a shell continuation prompt.
    """

    def __init__(self, pty_path, transcript_path):
        self.fd = os.open(pty_path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        try:
            tty.setraw(self.fd)
        except termios.error:
            pass
        self.buf = b''
        self.tf = open(transcript_path, 'ab', buffering=0)

    def close(self):
        try:
            os.close(self.fd)
        except OSError:
            pass
        self.tf.close()

    def _pump(self, timeout):
        r, _, _ = select.select([self.fd], [], [], timeout)
        if self.fd in r:
            try:
                data = os.read(self.fd, 65536)
            except OSError:
                return
            if data:
                self.buf += data
                self.tf.write(data)

    def expect(self, pattern, timeout):
        deadline = time.time() + timeout
        while True:
            m = pattern.search(self.buf)
            if m:
                self.buf = self.buf[m.end():]
                return m
            remaining = deadline - time.time()
            if remaining <= 0:
                return None
            self._pump(min(remaining, 1.0))

    def send(self, s):
        data = s.encode() if isinstance(s, str) else s
        self.tf.write(b'\n>>> SEND: ' + data + b'\n')
        off = 0
        while off < len(data):
            chunk = data[off:off + 16]
            try:
                n = os.write(self.fd, chunk)
            except BlockingIOError:
                select.select([], [self.fd], [], 1.0)
                continue
            off += n
            time.sleep(0.01)

    def run_cmd(self, label, cmd, timeout=90, _resends=0):
        self.buf = b''
        self.send(cmd + PEER_MARK_CMD + '\n')
        deadline = time.time() + timeout
        last_len, last_activity, nudges = 0, time.time(), 0
        while True:
            if re.search(rb'\r\n> $', self.buf) and _resends < 2:
                logging.info('peer %s: continuation prompt, ctrl-c + resend', label)
                self.send(b'\x03')
                self.expect(PEER_PROMPT, 15)
                time.sleep(1)
                return self.run_cmd(label, cmd, timeout, _resends + 1)
            m = PEER_MARK_RE.search(self.buf)
            if m:
                out = self.buf[:m.start()]
                self.buf = self.buf[m.end():]
                rc = int(m.group(1))
                self.expect(PEER_PROMPT, 10)
                text = out.decode('utf-8', 'replace')
                logging.info('peer %s -> rc=%d', label, rc)
                return rc, text
            now = time.time()
            if now >= deadline:
                raise PeerSetupError('peer command {!r} timed out'.format(label))
            if len(self.buf) != last_len:
                last_len, last_activity = len(self.buf), now
            elif now - last_activity > 10 and nudges < 3:
                nudges += 1
                logging.info('peer %s: no activity 10s, nudge %d', label, nudges)
                self.send('\r')
                last_activity = now
            self._pump(1.0)


class RenodeRun:
    def __init__(self, args, run_idx):
        self.args = args
        self.run_idx = run_idx
        self.workdir = args.workdir
        self.flash_file = os.path.join(args.workdir, 'flash.bin')
        self.peer_rootfs = os.path.join(args.workdir, 'peer-rootfs.jffs2')
        self.peer_pty = os.path.join(args.workdir, 'peer.pty')
        self.peer_transcript = os.path.join(args.workdir, 'peer-console.log')
        self.console_log = os.path.join(args.workdir, 'console.log')
        self.kernel_log = os.path.join(args.workdir, 'kernel.log')
        self.run_log = os.path.join(args.workdir, 'run.log')
        self.proc = None
        self.stdin_r = None
        self.run_log_f = None
        self.peer = None
        self.tail = ConsoleTail(self.console_log)
        self.pending = []
        self.t0 = None

    def milestone(self, msg):
        elapsed = 0.0 if self.t0 is None else time.time() - self.t0
        logging.info('[%8.1fs] %s', elapsed, msg)

    def launch(self):
        for stale in (self.console_log, self.kernel_log, self.run_log,
                      self.peer_transcript):
            if os.path.exists(stale):
                os.remove(stale)
        if os.path.lexists(self.peer_pty):
            os.remove(self.peer_pty)
        # Per-run scratch rootfs: the peer's CFI flash writes back into it.
        shutil.copyfile(PEER_ROOTFS_SRC, self.peer_rootfs)
        self.run_log_f = open(self.run_log, 'wb')
        monitor_setup = '; '.join([
            '$image_dir=@{}'.format(self.args.image_dir),
            '$flash_file=@{}'.format(self.flash_file),
            '$console_log=@{}'.format(self.console_log),
            '$kernel_log=@{}'.format(self.kernel_log),
            '$peer_bin=@{}'.format(PEER_BIN),
            '$peer_rootfs=@{}'.format(self.peer_rootfs),
            '$peer_pty=@{}'.format(self.peer_pty),
            'i @{}'.format(self.args.resc),
        ])
        cmd = [self.args.renode, '--disable-gui', '--console', '-e', monitor_setup]
        self.t0 = time.time()
        # Keep a stdin pipe open so Renode never sees EOF and exits.
        self.stdin_r, _stdin_w = os.pipe()
        self._stdin_w = _stdin_w
        self.proc = subprocess.Popen(
            cmd, stdin=self.stdin_r, stdout=self.run_log_f,
            stderr=subprocess.STDOUT, cwd=REPO_ROOT)
        if self.args.pid_file:
            with open(self.args.pid_file, 'w') as f:
                f.write(str(self.proc.pid))
        self.milestone('renode launched (pid {})'.format(self.proc.pid))

    def check_alive(self):
        if self.proc.poll() is not None:
            raise RunFailure('renode exited prematurely (code {}); see {}'.format(
                self.proc.returncode, self.run_log))

    def check_panic(self):
        if self.tail.panic_lines:
            raise RunFailure('PANIC in PID: {}'.format(
                self.tail.panic_lines[-1].strip()))

    def bring_up_peer(self):
        """Log in to the peer and provision its test services and DHCP options.
        All machines start together (the resc), but the peer boots and is
        provisioned faster than the DUT reaches DHCP, and the udhcpd/DNS
        reconfigure below runs first to widen that margin, so the DUT's lease
        sees the resolver pointing at the peer's dnsd."""
        for _ in range(int(self.args.timeout_peer / 0.5)):
            if os.path.lexists(self.peer_pty):
                break
            self.check_alive()
            time.sleep(0.5)
        else:
            raise PeerSetupError('peer pty never appeared')
        self.peer = PeerConsole(self.peer_pty, self.peer_transcript)
        if self.peer.expect(PEER_BANNER, self.args.timeout_peer) is None:
            raise PeerSetupError('peer console banner never appeared')
        self.milestone('peer console banner')
        time.sleep(2)
        self.peer.buf = b''
        self.peer.send('\n')
        if self.peer.expect(PEER_PROMPT, 60) is None:
            raise PeerSetupError('peer shell prompt never appeared after Enter')
        self.milestone('peer root shell active')
        # Deterministic ordering without machine control: kill the bogus udhcpd
        # rcS started FIRST so nothing answers while we provision; the DUT's
        # DISCOVERs retry until the *reconfigured* udhcpd starts LAST, once every
        # other service is up. Removes the bogus-DNS and services-not-ready races.
        self.peer.run_cmd('stop_dhcp_and_remount',
                          'pkill udhcpd 2>/dev/null; '
                          'mount -o remount,rw /dev/root / && mkdir -p /tmp && echo RW_OK')
        # dnsd: static A records bound to the peer's own address.
        records = ''.join('{} {}\\n'.format(name, ip) for name, ip in PEER_DNS_RECORDS)
        self.peer.run_cmd('start_dnsd',
                          'printf "{}" > /tmp/dnsd.conf && '
                          'dnsd -c /tmp/dnsd.conf -i {} -p 53 -d & sleep 1; echo DNS_UP'
                          .format(records, PEER_IP))
        # TCP echo (persistent) and UDP echo (one-shot -l: busybox `nc -u -lk`
        # is unreliable; a single -l session handles the test's datagram burst).
        self.peer.run_cmd('start_tcp_echo',
                          'nc -lk -p {} -e /bin/cat </dev/null & sleep 1; echo TCP_UP'
                          .format(PEER_ECHO_TCP))
        self.peer.run_cmd('start_udp_echo',
                          'nc -u -l -p {} -e /bin/cat </dev/null & sleep 1; echo UDP_UP'
                          .format(PEER_ECHO_UDP))
        # Bulk source: serve PEER_BULK_LEN bytes of 'A' (one-directional). Split
        # into short commands -- long lines are what the uart occasionally drops.
        self.peer.run_cmd('make_bulk_file',
                          'head -c {} /dev/zero | tr "\\000" "A" > /tmp/bulk.bin; echo MK_OK'
                          .format(PEER_BULK_LEN))
        self.peer.run_cmd('start_bulk_source',
                          'nc -lk -p {} < /tmp/bulk.bin & echo BULK_UP'.format(PEER_BULK_TCP))
        # LAST: rewrite udhcpd's DNS option to advertise the peer as resolver and
        # start it. This is now the only DHCP server on the switch, so the DUT
        # binds this config (resolver -> the peer's dnsd) with every service up.
        self.peer.run_cmd('start_udhcpd',
                          'sed -i "s/^opt.*dns.*/opt dns {}/I" /etc/udhcpd.conf 2>/dev/null; '
                          'grep -qi "opt dns" /etc/udhcpd.conf || '
                          'echo "opt dns {}" >> /etc/udhcpd.conf; '
                          'udhcpd /etc/udhcpd.conf & sleep 1; echo DHCP_UP'
                          .format(PEER_IP, PEER_IP))
        self.milestone('peer services up (dnsd, tcp {}, udp {}, bulk {}); udhcpd started last'
                       .format(PEER_ECHO_TCP, PEER_ECHO_UDP, PEER_BULK_TCP))

    def shutdown(self):
        try:
            if self.proc is not None and self.proc.poll() is None:
                try:
                    os.write(self._stdin_w, b'quit\n')
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
            if self.proc is not None and self.proc.poll() is None:
                try:
                    self.proc.kill()
                    self.proc.wait()
                except OSError:
                    pass
            if self.peer is not None:
                self.peer.close()
            for fd in (getattr(self, '_stdin_w', None), self.stdin_r):
                if fd is not None:
                    try:
                        os.close(fd)
                    except OSError:
                        pass
            if self.run_log_f is not None:
                self.run_log_f.close()
            self.tail.close()
            if self.args.pid_file and os.path.exists(self.args.pid_file):
                os.remove(self.args.pid_file)

    def poll_console(self):
        new_lines = self.tail.poll()
        for line in new_lines:
            logging.debug('console: %s', line)
        self.pending.extend(new_lines)

    def wait_for(self, markers, timeout):
        deadline = time.time() + timeout
        while time.time() < deadline:
            self.check_alive()
            self.poll_console()
            self.check_panic()
            while self.pending:
                line = self.pending.pop(0)
                for marker in markers:
                    if marker in line:
                        self.milestone('marker: {}'.format(marker))
                        return marker
            time.sleep(0.2)
        raise RunFailure('timeout ({} s) waiting for {}'.format(
            timeout, ' | '.join(markers)))

    def collect_results(self):
        problems = []
        counts = {'PASS': 0, 'FAIL': 0, 'XFAIL': 0, 'XPASS': 0}
        done = None
        last_sentinel = None
        trailing = []
        last_activity = time.time()
        deadline = time.time() + self.args.timeout_tests
        while done is None:
            now = time.time()
            if now >= deadline:
                problems.append('overall test cap hit: no NET-TESTS DONE within '
                                '{} s (last: {})'.format(self.args.timeout_tests,
                                                         last_sentinel or 'none'))
                break
            if now - last_activity >= self.args.timeout_inactivity:
                problems.append(
                    'console INACTIVE for {} s -- net service presumed dead. '
                    'Last TEST sentinel: {}. Trailing:\n  {}'.format(
                        self.args.timeout_inactivity,
                        last_sentinel or 'none',
                        '\n  '.join(trailing[-12:]) or '(none)'))
                break
            self.check_alive()
            before = len(self.pending)
            self.poll_console()
            self.check_panic()
            if len(self.pending) > before:
                last_activity = time.time()
            while self.pending:
                line = self.pending.pop(0)
                trailing.append(line)
                m = RE_TEST.search(line)
                if m:
                    name, verdict = m.group(1), m.group(2)
                    counts[verdict] = counts.get(verdict, 0) + 1
                    last_sentinel = '{} {}'.format(name, verdict)
                    self.milestone('TEST {} {}{}'.format(
                        name, verdict, (' ' + m.group(3)) if m.group(3) else ''))
                    continue
                m = RE_DONE.search(line)
                if m:
                    done = [int(x) for x in m.groups()]
                    self.milestone('NET-TESTS DONE: pass={} fail={} xfail={} '
                                   'xpass={} total={}'.format(*done))
                    break
            time.sleep(0.05)
        if done is not None:
            _, n_fail, _, n_xpass, n_total = done
            if n_fail:
                problems.append('DONE reports fail={}'.format(n_fail))
            if n_xpass:
                problems.append('DONE reports xpass={}'.format(n_xpass))
            seen = sum(counts.values())
            if seen != n_total:
                problems.append('sentinel count mismatch: saw {} TEST lines, '
                                'DONE says total={}'.format(seen, n_total))
        self.poll_console()
        self.check_panic()
        return problems


def write_blank_flash(path):
    with open(path, 'wb') as f:
        chunk = b'\xff' * FLASH_CHUNK
        for _ in range(FLASH_SIZE // FLASH_CHUNK):
            f.write(chunk)


def preserve_failure_logs(args, run_idx):
    dst = os.path.join(args.workdir, 'failed-run-{}'.format(run_idx))
    os.makedirs(dst, exist_ok=True)
    for name in ('console.log', 'kernel.log', 'run.log', 'peer-console.log'):
        src = os.path.join(args.workdir, name)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(dst, name))
    logging.info('failure logs preserved in %s', dst)


def do_run(args, run_idx):
    logging.info('===== run %d/%d =====', run_idx + 1, args.runs)
    # A peer-console flake during provisioning (a harness issue, not a DUT
    # result) retries the whole run once; a real test failure does not.
    for attempt in range(2):
        write_blank_flash(os.path.join(args.workdir, 'flash.bin'))
        run = RenodeRun(args, run_idx)
        try:
            run.launch()
            run.bring_up_peer()
            run.wait_for([MARK_BOOT], args.timeout_boot)
            run.wait_for([MARK_NET_OK], args.timeout_net_ready)
            problems = run.collect_results()
            if problems:
                for p in problems:
                    logging.error('run %d: %s', run_idx, p)
                preserve_failure_logs(args, run_idx)
                return run_idx, 'FAIL'
            logging.info('run %d PASS', run_idx)
            return run_idx, 'PASS'
        except PeerSetupError as e:
            if attempt == 0:
                logging.warning('run %d: peer setup flake (%s); retrying the run',
                                run_idx, e)
                continue
            logging.error('run %d: peer setup flake on retry (%s)', run_idx, e)
            preserve_failure_logs(args, run_idx)
            return run_idx, 'FAIL'
        except RunFailure as e:
            logging.error('run %d: %s', run_idx, e)
            preserve_failure_logs(args, run_idx)
            return run_idx, 'FAIL'
        finally:
            run.shutdown()


def main():
    p = argparse.ArgumentParser(description='Renode Cross-host tester for std::net')
    p.add_argument('--workdir',
                   default=os.path.join(REPO_ROOT, 'target', 'std-net-cross-host-ci'))
    p.add_argument('--resc',
                   default=os.path.join(REPO_ROOT, 'emulation', 'tests',
                                        'net-cross-host-ci.resc'))
    p.add_argument('--image-dir',
                   default=os.path.join(REPO_ROOT, 'target',
                                        'riscv32imac-unknown-xous-elf', 'release'))
    p.add_argument('--renode', default='renode')
    p.add_argument('--runs', type=int, default=1)
    p.add_argument('--timeout-peer', type=float, default=300.0,
                   help='wall seconds for the peer pty + banner + shell')
    p.add_argument('--timeout-boot', type=float, default=300.0)
    p.add_argument('--timeout-net-ready', type=float, default=400.0,
                   help='real DHCP can take longer than the loopback static seed')
    p.add_argument('--timeout-tests', type=float, default=1800.0)
    p.add_argument('--timeout-inactivity', type=float, default=180.0)
    p.add_argument('--pid-file')
    p.add_argument('--loglevel', default='INFO')
    args = p.parse_args()

    logging.basicConfig(level=getattr(logging, args.loglevel.upper()),
                        format='%(levelname)s:%(name)s:%(message)s')
    os.makedirs(args.workdir, exist_ok=True)

    results = [do_run(args, i) for i in range(args.runs)]
    failures = [i for i, verdict in results if verdict != 'PASS']
    if failures:
        logging.error('%d/%d runs FAILED: %s', len(failures), args.runs, failures)
        sys.exit(1)
    logging.info('all %d run(s) passed', args.runs)
    sys.exit(0)


if __name__ == '__main__':
    main()

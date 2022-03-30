#! /usr/bin/env python3
import argparse
import os
import sys
import logging
import subprocess
import time
import signal

def main():
    parser = argparse.ArgumentParser(description="Regression tester for PDDB")
    parser.add_argument(
        "--name", required=True, help="pddb disk image root name", type=str, nargs='?', metavar=('name'), const='./pddb'
    )
    parser.add_argument(
        "--loglevel", required=False, help="set logging level (INFO/DEBUG/WARNING/ERROR)", type=str, default="INFO",
    )
    parser.add_argument(
        "--runs", required=False, help="sets the number of runs", default='501'
    )
    args = parser.parse_args()

    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    pass_log = {}
    err_log = []
    timeout = 240 # a bit longer to allow for a compilation to happen
    for seed in range(0, int(args.runs)):
        # remove the previous runs analysis file
        try:
            os.remove('./tools/pddb-images/{}.bin'.format(args.name))
            os.remove('./tools/pddb-images/{}.key'.format(args.name))
        except OSError as e:
            print('Error removing previous run output: {}'.format(e.strerror))

        err_log.append("Starting seed {}".format(seed))
        seed_env = os.environ
        seed_env["XOUS_SEED"] = str(seed)
    #    result = subprocess.run(['tools/pddbdbg.py', '--name', 'patterne'], env=seed_env, capture_output=True, text=True, universal_newlines=True)
        #result = subprocess.run(['cargo', 'xtask', 'run'], env=seed_env, capture_output=True, text=True, universal_newlines=True)
        #for line in result.stdout.split('\n'):
        #    if 'Seed' in line:
        #        logging.info(line)

        passing = 'PASS'
        proc = subprocess.Popen(
            ['cargo', 'xtask', 'pddb-ci'],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            #shell=True,
            encoding='utf-8',
            errors='replace'
        )
        start_time = time.time()
        while True:
            realtime_output = proc.stdout.readline()
            if (realtime_output == '' and proc.poll() is not None) or (time.time() - start_time > 20):
                proc.kill()
                if time.time() - start_time > timeout:
                    logging.debug("timeout on generation")
                    passing = 'FAIL TIMEOUT'
                break
            if realtime_output:
                if 'Seed' in realtime_output:
                    logging.info(realtime_output.strip()) # flush=True for print version
                if 'CI done' in realtime_output:
                    logging.info(realtime_output.strip())
                    proc.kill()
                if 'Ran out of memory' in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("ran out of space")
                    # passing = False # not a fail, because it's the test condition that's wrong, not the code
                    passing = 'OOM'
                    proc.kill()
                if "couldn't allocate memory" in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("ran out of space")
                    # passing = False # not a fail, because it's the test condition that's wrong, not the code
                    passing = 'OOM'
                    proc.kill()
                if "No free space" in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("ran out of space")
                    # passing = False # not a fail, because it's the test condition that's wrong, not the code
                    passing = 'OOM'
                    proc.kill()
                if 'Decryption auth error' in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("decryption auth error")
                    passing = 'FAIL AUTH'
                    proc.kill()

        timeout = 20 # reset the timeout after the first run to something shorter, now that any potential compilation step is done

        proc = subprocess.Popen(
            [sys.executable, './tools/pddbdbg.py', '--name', args.name],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            encoding='utf-8',
            errors='replace'
        )
        start_time = time.time()
        script_success=False
        while True:
            realtime_output = proc.stdout.readline()
            if (realtime_output == '' and proc.poll() is not None) or (time.time() - start_time > 45):
                if time.time() - start_time > 45:
                    passing = 'FAIL ANALYSIS TIMEOUT'
                    logging.debug("analysis timed out")
                proc.kill()
                break
            if realtime_output:
                if 'ERROR' in realtime_output:
                    logging.info(realtime_output.strip())
                    err_log.append(realtime_output)
                    logging.debug("output contained errors")
                    passing = 'FAIL CI ERRORS'
                if 'WARNING' in realtime_output:
                    logging.info(realtime_output.strip())
                    err_log.append(realtime_output)
                    logging.debug("output contained warnings")
                    passing = 'FAIL CI WARNINGS'
                if 'All dicts were found.' in realtime_output:
                    script_success=True

        if passing == 'PASS' and script_success == False:
            passing = 'FAIL CI COULD NOT RUN'

        logging.info("Seed {} {}".format(seed, passing))
        pass_log[seed] = passing

        for line in err_log:
            logging.info(line)
        err_log = []

    # summary report
    passing = True
    for items in pass_log.items():
        logging.info(items)
        if items[1] != 'PASS':
            passing = False
    if passing:
        logging.info("Overall pass, exiting with 0")
        exit(0)
    else:
        logging.info("A failure was detected, exiting with 1")
        exit(1)

if __name__ == "__main__":
    main()

#! /usr/bin/env python3
import argparse
import os
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
    args = parser.parse_args()

    numeric_level = getattr(logging, args.loglevel.upper(), None)
    if not isinstance(numeric_level, int):
        raise ValueError('Invalid log level: %s' % args.loglevel)
    logging.basicConfig(level=numeric_level)

    pass_log = {}
    err_log = []
    for seed in range(0, 201):
        err_log.append("Starting seed {}".format(seed))
        seed_env = os.environ
        seed_env["XOUS_SEED"] = str(seed)
    #    result = subprocess.run(['tools/pddbdbg.py', '--name', 'patterne'], env=seed_env, capture_output=True, text=True, universal_newlines=True)
        #result = subprocess.run(['cargo', 'xtask', 'run'], env=seed_env, capture_output=True, text=True, universal_newlines=True)
        #for line in result.stdout.split('\n'):
        #    if 'Seed' in line:
        #        logging.info(line)

        start_time = time.time()
        passing = True
        proc = subprocess.Popen(
            ['cargo', 'xtask', 'run'],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            #shell=True,
            encoding='utf-8',
            errors='replace'
        )
        while True:
            realtime_output = proc.stdout.readline()
            if (realtime_output == '' and proc.poll() is not None) or (time.time() - start_time > 20):
                proc.kill()
                if time.time() - start_time > 20:
                    logging.debug("timeout on generation")
                    passing = False
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
                    proc.kill()
                if "couldn't allocate memory" in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("ran out of space")
                    # passing = False # not a fail, because it's the test condition that's wrong, not the code
                    proc.kill()
                if 'Decryption auth error' in realtime_output:
                    err_log.append(realtime_output)
                    logging.debug("decryption auth error")
                    passing = False
                    proc.kill()

        proc = subprocess.Popen(
            ['python', './tools/pddbdbg.py', '--name', args.name],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            encoding='utf-8',
            errors='replace'
        )
        while True:
            realtime_output = proc.stdout.readline()
            if (realtime_output == '' and proc.poll() is not None) or (time.time() - start_time > 45):
                if time.time() - start_time > 45:
                    passing = False
                    logging.debug("analysis timed out")
                proc.kill()
                break
            if realtime_output:
                if 'ERROR' in realtime_output:
                    logging.info(realtime_output.strip())
                    err_log.append(realtime_output)
                    logging.debug("output contained errors")
                    passing = False
                if 'WARNING' in realtime_output:
                    logging.info(realtime_output.strip())
                    err_log.append(realtime_output)
                    logging.debug("output contained warnings")
                    passing = False

        if passing:
            logging.info("Seed {} PASS".format(seed))
            pass_log[seed] = 'PASS'
        else:
            logging.info("Seed {} FAIL".format(seed))
            pass_log[seed] = 'FAIL'

    for line in err_log:
        logging.info(line)

    for items in pass_log.items():
        logging.info(items)


if __name__ == "__main__":
    main()
    exit(0)

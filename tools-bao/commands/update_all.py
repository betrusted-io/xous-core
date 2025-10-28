import json
import sys
import logging
from utils.common import project_root
from utils.version_utils import read_local_versions, query_board_versions


def cmd_update_all(args) -> None:
    root = project_root()

    local_semver, local_ts = read_local_versions(root)
    if not local_semver:
        logging.error("[bao] could not parse SEMVER from services/xous-ticktimer/src/version.rs")
        sys.exit(2)

    board_semver, board_ts = query_board_versions(args.port, args.baud, timeout_s=args.timeout)
    if board_semver is None:
        logging.error("[bao] could not read board version (is the board connected and running?)")
        sys.exit(2)

    # Decision uses SEMVER vs SEMVER (timestamps are logged but not decisive)
    try:
        needs = (local_semver.strip() != board_semver.strip())
    except AttributeError:
        # If the board didn't report a version, assume an update is needed
        needs = True

    if args.json:
        print(json.dumps({
            "updateAll": needs,
            "localSemver": local_semver,
            "localTimestamp": local_ts,
            "boardSemver": board_semver,
            "boardTimestamp": board_ts
        }))
    else:
        print("true" if needs else "false")
    sys.exit(0)

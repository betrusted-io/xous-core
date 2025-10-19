import json, sys
from pathlib import Path
from utils.version_utils import read_local_versions, query_board_versions

def cmd_update_all(args):
    root = Path(__file__).resolve().parents[2]

    local_semver, local_ts = read_local_versions(root)
    if not local_semver:
        print("[bao] could not parse SEMVER from services/xous-ticktimer/src/version.rs", file=sys.stderr)
        sys.exit(2)

    board_semver, board_ts = query_board_versions(args.port, args.baud, timeout_s=args.timeout)
    if board_semver is None:
        print("[bao] could not read board version (is the board connected and running?)", file=sys.stderr)
        sys.exit(2)

    # Decision uses SEMVER vs SEMVER (timestamps are logged but not decisive)
    needs = (local_semver.strip() != board_semver.strip())

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

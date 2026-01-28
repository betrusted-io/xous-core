#! /usr/bin/env python3
import argparse
from pathlib import Path
import re
import subprocess
import time

# format is [crate : path]
# this is an ordered list that also prescribes the publication order to crates.io
UTRA_CRATES = [
    ["svd2utra", "svd2utra"],
    ["utralib", "utralib"],
]
CRATES = [
    # ["xous", "xous-rs"],
    # ["xous-ipc", "xous-ipc"],
    # ["xous-api-log", "api/xous-api-log"],
    # ["xous-api-names", "api/xous-api-names"],
    ["xous-api-ticktimer", "api/xous-api-ticktimer"],

    # ["xous-api-susres", "api/xous-api-susres"],
    # ["xous-kernel", "kernel"], # this is no longer published, as it is an implementation
    # ["xous-log", "services/xous-log"],  # implementations, no longer published
    # ["xous-names", "services/xous-names"],
    # ["xous-susres", "services/xous-susres"],
    # ["xous-ticktimer", "services/xous-ticktimer"],
]
# dictionary of crate names -> version strings
VERSIONS = {}

class PatchInfo:
    def __init__(self, filename, cratelist=None, cratename=None):
        self.filepath = Path(filename)
        if not self.filepath.is_file():
            print("Bad crate path: {}".format(filename))
        self.cratename = cratename
        self.cratelist = cratelist
        self.debug = False

    def get_version(self):
        with open(self.filepath, 'r') as file:
            lines = file.readlines()
            in_package = False
            name_check = False
            for line in lines:
                if line.strip().startswith('['):
                    if 'package' in line:
                        in_package = True
                    else:
                        in_package = False
                    continue
                elif in_package:
                    if line.strip().startswith('version'):
                        version = line.split('=')[1].replace('"', '').strip()
                    if line.strip().startswith('name'):
                        name = line.split('=')[1].replace('"', '').strip()
                        for [item_name, path] in self.cratelist:
                            if name == item_name:
                                name_check = True
            if name_check:
                assert version is not None # "Target name found but no version was extracted!"
                VERSIONS[name] = version

            return name_check

    def debug_mode(self, arg):
        self.debug = arg

    def output(self, line):
        if self.debug:
            print("Dry run: {}".format(line.rstrip()))
            # pass
        else:
            self.file.write(line)

    # assumes that VERSIONS has been initialized.
    def increment_versions(self, mode='bump'):
        # check that global variables are in sane states
        assert len(VERSIONS) > 0 # "No VERSIONS found, something is weird."
        with open(self.filepath, 'r') as file:
            lines = file.readlines()
        with open(self.filepath, 'w', newline='\n') as file:
            self.file = file
            in_package = False
            in_dependencies = False
            for line in lines:
                if line.strip().startswith('['):
                    if 'package' in line:
                        in_package = True
                    else:
                        in_package = False
                    if 'dependencies' in line:
                        in_dependencies = True
                    else:
                        in_dependencies = False
                    self.output(line)
                elif line.strip().startswith('#'): # skip comments
                    self.output(line)
                elif in_package:
                    # increment my own version, if I'm in the listed crates and we're in 'bump' mode
                    if (self.cratename is not None) and (mode == 'bump'):
                        if line.strip().startswith('version'):
                            self.output('version = "{}"\n'.format(bump_version(VERSIONS[self.cratename])))
                        else:
                            self.output(line)
                    else:
                        self.output(line)

                    if line.strip().startswith('name'):
                        this_crate = line.split('=')[1].replace('"', '').strip()
                        print("Patching {}...".format(this_crate))
                elif in_dependencies:
                    # now increment dependency versions

                    # first search and see if the dependency name is in the table
                    if 'package' in line: # renamed package
                        semiparse = re.split('=|,', line.strip())
                        if 'package' in semiparse[0]:
                            # catch the case that the crate name has the word package in it. If this error gets printed,
                            # we have to rewrite this semi-parser to be smarter about looking inside the curly braces only,
                            # instead of just stupidly splitting on = and ,
                            print("Warning: crate name has the word 'package' in it, and we don't parse this correctly!")
                            print("Searching in {}, found {}".format(this_crate, semiparse[0]))
                        else:
                            # extract the first index where 'package' is found
                            index = [idx for idx, s in enumerate(semiparse) if 'package' in s][0]
                            depcrate = semiparse[index + 1].replace('"', '').strip()
                            # print("assigned package name: {}".format(depcrate))
                    else: # simple version number
                        depcrate = line.strip().split('=')[0].strip()
                        # print("simple package name: {}".format(depcrate))

                    # print("{}:{}".format(this_crate, depcrate))
                    if depcrate in VERSIONS:
                        if mode == 'bump':
                            oldver = VERSIONS[depcrate]
                            (newline, numsubs) = re.subn(oldver, bump_version(oldver), line)
                            if numsubs != 1 and not "\"*\"" in newline:
                                print("Warning! Version substitution failed for {}:{} in crate {} ({})".format(depcrate, oldver, this_crate, numsubs))

                            # print("orig: {}\nnew: {}".format(line, newline))
                            self.output(newline)
                        elif mode == 'to_local':
                            if 'path' in line:
                                self.output(line) # already local path, do nothing
                            else:
                                for [name, path] in self.cratelist:
                                    if depcrate == name:
                                        # print("self.file: {}".format(self.file.name))
                                        depth = self.file.name.count('/')
                                        base = '../' * (depth)
                                        subpath = 'path = "{}{}"'.format(base, path)
                                if subpath is None:
                                    print("Error: couldn't find substitution path for dependency {}".format(depcrate))

                                if 'version' in line:
                                    oldver = 'version = "{}"'.format(VERSIONS[depcrate])
                                    newpath = subpath
                                else:
                                    oldver = '"{}"'.format(VERSIONS[depcrate])
                                    newpath = '{{ {} }}'.format(subpath)

                                (newline, numsubs) = re.subn(oldver, newpath, line)
                                if numsubs != 1 and not "\"*\"" in newline:
                                    print("Warning! Path substitution failed for {}:{} in crate {} ({})".format(depcrate, oldver, this_crate, numsubs))

                                self.output(newline)
                        elif mode == 'to_remote':
                            if 'path' not in line:
                                self.output(line) # already remote, nothing to do
                            else:
                                if '{' in line and ',' in line:
                                    specs = re.split(',|}|{', line) # line.split(',')
                                    for spec in specs:
                                        if 'path' in spec:
                                            oldpath = spec.rstrip().lstrip()
                                    if oldpath is None:
                                        print("Error! couldn't parse out path to substitute for dependency {}".format(depcrate))
                                    newver = 'version = "{}"'.format(VERSIONS[depcrate])
                                    (newline, numsubs) = re.subn(oldpath, newver, line)
                                    if numsubs != 1 and not "\"*\"" in newline:
                                        print("Warning! Path substitution failed for {}:{} in crate {} ({})".format(depcrate, oldpath, this_crate, numsubs))
                                    else:
                                        # print("Substitute {}:{} in crate {} ({})".format(depcrate, oldpath, this_crate, newver))
                                        # print("  " + oldpath)
                                        # print("  " + newline)
                                        pass
                                    self.output(newline)
                                else:
                                    self.output('{} = "{}"\n'.format(depcrate, VERSIONS[depcrate]))
                    else:
                        self.output(line)
                else:
                    self.output(line)
                # if debug mode, just write the line unharmed
                if self.debug:
                    self.file.write(line)

def bump_version(semver):
    components = semver.split('.')
    components[-1] = str(int(components[-1]) + 1)
    retver = ""
    for (index, component) in enumerate(components):
        retver += str(component)
        if index < len(components) - 1:
            retver += "."
    return retver

def main():
    parser = argparse.ArgumentParser(description="Update and publish crates")
    parser.add_argument(
        "-x", "--xous", help="Process Xous kernel dependent crates", action="store_true",
    )
    parser.add_argument(
        "-u", "--utralib", help="Process UTRA dependent crates", action="store_true",
    )
    parser.add_argument(
        "-b", "--bump", help="Do a version bump", action="store_true",
    )
    parser.add_argument(
        "-p", "--publish", help="Publish crates", action="store_true",
    )
    parser.add_argument(
        "-l", "--local-paths", help="Convert crate references to local paths", action="store_true"
    )
    parser.add_argument(
        "-r", "--remote-paths", help="Convert crate references to remote paths", action="store_true"
    )
    parser.add_argument(
        "-w", "--wet-run", help="Used in conjunction with --publish to do a 'wet run'", action="store_true"
    )
    args = parser.parse_args()

    if not(args.xous or args.utralib):
        print("Warning: no dependencies selected, operation is a no-op. Use -x/-u/... to select dependency trees")
        exit(1)

    cratelist = []
    if args.utralib: # ordering is important, the UTRA crates need to publish before Xous crates
        cratelist += UTRA_CRATES
        if not args.xous: # most Xous crates are also affected by this, so they need a bump as well
            cratelist += CRATES
    if args.xous:
        cratelist += CRATES

    if (args.bump or args.publish) and (args.local_paths or args.remote_paths):
        print("Do not mix path changes with bump and publish operations. Do them serially.")
        exit(1)
    if args.local_paths and args.remote_paths:
        print("Can't simultaneously change to local and remote paths. Pick only one operation.")
        exit(1)

    crate_roots = ['.', '../hashes/sha2', '../curve25519-dalek/curve25519-dalek']

    if args.bump or args.local_paths or args.remote_paths:
        cargo_toml_paths = []
        for roots in crate_roots:
            for path in Path(roots).rglob('Cargo.toml'):
                if 'target' not in str(path):
                    not_core_path = True
                    for cratespec in cratelist:
                        editpath = cratespec[1]
                        normpath = str(Path(path)).replace('\\', '/').rpartition('/')[0]  # fix windows paths
                        # print(editpath)
                        # print(normpath)
                        if editpath == normpath:
                            not_core_path = False
                    if not_core_path:
                        cargo_toml_paths += [path]

        # import pprint
        # pp = pprint.PrettyPrinter(indent=2)
        # pp.pprint(cargo_toml_paths)

        patches = []
        # extract the versions of crates to patch
        for [crate, path] in cratelist:
            #print("extracting {}".format(path))
            patchinfo = PatchInfo(path + '/Cargo.toml', cratelist, crate)
            if not patchinfo.get_version():
                print("Couldn't extract version info from {} crate".format(crate))
                exit(1)
            patches += [patchinfo]

        # now extract all the *other* crates
        for path in cargo_toml_paths:
            #print("{}".format(str(path)))
            patchinfo = PatchInfo(path, cratelist)
            patches += [patchinfo]

        if args.bump:
            for (name, ver) in VERSIONS.items():
                print("{}: {} -> {}".format(name, ver, bump_version(ver)))
        if args.local_paths or args.remote_paths:
            print("Target crate list")
            for (name, ver) in VERSIONS.items():
                print("{}: {}".format(name, ver))

        if args.bump:
            mode = 'bump'
        elif args.local_paths:
            mode = 'to_local'
        elif args.remote_paths:
            mode = 'to_remote'

        for patch in patches:
            patch.debug_mode(not args.wet_run)
            patch.increment_versions(mode)
        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        print("Don't forget to check in & update git rev in Cargo.lock for: {}".format(crate_roots[1:]))
        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")

    if args.publish:
        # small quirk: if you're doing a utralib update, just use -u only.
        # there is some order-sensitivity in how the dictionaries are accessed
        # but of course dictionaries are unordered. I think we need to re-do
        # the specifier from a dictionary to an ordered list, to guarantee that
        # publishing happens in the correct operation order.
        wet_cmd = ["cargo",  "publish"]
        dry_cmd = ["cargo",  "publish", "--dry-run", "--allow-dirty"]
        if args.wet_run:
            cmd = wet_cmd
        else:
            cmd = dry_cmd
        for [crate, path] in cratelist:
            print("Publishing {} in {}".format(crate, path))
            try:
                subprocess.run(cmd, cwd=path, check=True, capture_output=True, encoding='utf-8')
            except subprocess.CalledProcessError as err:
                if 'already uploaded' in err.stderr:
                    print("  Already uploaded, skipping to next module...")
                    continue

                print("Process failed, waiting for crates.io to update and retrying...")
                time.sleep(2) # the latest Cargo seems to fix this problem
                # just try running it again
                try:
                    subprocess.run(cmd, cwd=path, check=True)
                except:
                    print("Retry failed, moving on anyways...")
            time.sleep(1)

        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")
        print("Don't forget to manually push alternate crate roots to github: {}".format(crate_roots[1:]))
        print("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!")

if __name__ == "__main__":
    main()
    exit(0)

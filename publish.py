#! /usr/bin/env python3
import argparse
from pathlib import Path, PurePath
import re

# format is {crate : path}
CRATES = {
    "xous" : "xous-rs",
    "xous-kernel" : "kernel",
    "xous-ipc" : "xous-ipc",
    "xous-api-log" : "api/xous-api-log",
    "xous-api-names" : "api/xous-api-names",
    "xous-api-susres" : "api/xous-api-susres",
    "xous-api-ticktimer" : "api/xous-api-ticktimer",
    "xous-log" : "services/xous-log",
    "xous-names" : "services/xous-names",
    "xous-susres" : "services/xous-susres",
    "xous-ticktimer" : "services/xous-ticktimer",
}
VERSIONS = {}

class PatchInfo:
    def __init__(self, filename, cratename=None):
        self.filepath = Path(filename)
        if not self.filepath.is_file():
            print("Bad crate path: {}".format(filename))
        self.cratename = cratename

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
                        if name in CRATES:
                            name_check = True
            if name_check:
                assert(version is not None, "Target name found but no version was extracted!")
                VERSIONS[name] = version

            return name_check

    # assumes that VERSIONS has been initialized.
    def increment_versions(self):
        # check that global variables are in sane states
        assert(len(VERSIONS) > 0, "No VERSIONS found, something is weird.")
        assert(len(VERSIONS) == len(CRATES), "Not all VERSIONS were extracted. You probably didn't mean to do this.")
        with open(self.filepath, 'r') as file:
            lines = file.readlines()
        with open(self.filepath, 'w') as file:
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
                    file.write(line)
                elif line.strip().startswith('#'): # skip comments
                    file.write(line)
                elif in_package:
                    # increment my own version, if I'm in the listed crates
                    if self.cratename is not None:
                        if line.strip().startswith('version'):
                            file.write('version = "{}"\n'.format(bump_version(VERSIONS[self.cratename])))
                        else:
                            file.write(line)
                    else:
                        file.write(line)

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
                        oldver = VERSIONS[depcrate]
                        (newline, numsubs) = re.subn(oldver, bump_version(oldver), line)
                        if numsubs != 1 and not "\"*\"" in newline:
                            print("Warning! Version substitution failed for {}:{} in crate {} ({})".format(depcrate, oldver, this_crate, numsubs))

                        # print("orig: {}\nnew: {}".format(line, newline))
                        file.write(newline)
                    else:
                        file.write(line)
                else:
                    file.write(line)

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
        "-b", "--bump-versions", help="Bump version numbers on all affected crates", action="store_true",
    )
    parser.add_argument(
        "-p", "--publish", help="Publish crates", action="store_true",
    )
    args = parser.parse_args()

    if args.bump_versions:
        cargo_toml_paths = []
        for path in Path('.').rglob('Cargo.toml'):
            if 'target' not in str(path):
                not_core_path = True
                for editpath in CRATES.values():
                    if editpath in str(Path(path)).replace('\\', '/'): # fix windows paths
                        not_core_path = False
                if not_core_path:
                    cargo_toml_paths += [path]

        #import pprint
        #pp = pprint.PrettyPrinter(indent=2)
        #pp.pprint(cargo_toml_paths)

        patches = []
        # extract the versions of crates to patch
        for (crate, path) in CRATES.items():
            #print("extracting {}".format(path))
            patchinfo = PatchInfo(path + '/Cargo.toml', crate)
            if not patchinfo.get_version():
                print("Couldn't extract version info from {} crate".format(crate))
                exit(1)
            patches += [patchinfo]

        # now extract all the *other* crates
        for path in cargo_toml_paths:
            #print("{}".format(str(path)))
            patchinfo = PatchInfo(path)
            patches += [patchinfo]

        print(VERSIONS)
        for (name, ver) in VERSIONS.items():
            print("{}: {} -> {}".format(name, ver, bump_version(ver)))

        for patch in patches:
            patch.increment_versions()

if __name__ == "__main__":
    main()
    exit(0)

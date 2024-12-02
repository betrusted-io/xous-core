#! /usr/bin/env python3

import argparse
import json
import sys
import os
import os.path
import re


class Main(object):
    def __init__(self):
        parser = argparse.ArgumentParser(description="Xous i18n Helper")
        parser.add_argument(
            "-v",
            "--verbose",
            help="Prints details of each action",
            action="store_true",
            required=False,
        )
        parser.add_argument(
            "-l",
            "--list-languages",
            help="Lists current translations",
            action="store_true",
            required=False,
        )
        parser.add_argument(
            "-i",
            "--list-i18n-files",
            help="Lists i18n files",
            action="store_true",
            required=False,
        )
        parser.add_argument(
            "-m",
            "--missing",
            help="Shows missing translations",
            action="store_true",
            required=False,
        )
        parser.add_argument(
            "-o",
            "--show-ok",
            help="Shows OK translations",
            action="store_true",
            required=False,
        )
        parser.add_argument(
            "-n", "--new-lang", help="Add support for a new lang", required=False
        )
        parser.add_argument(
            "-f",
            "--from-lang",
            help="Copy this existing lang for the new lang",
            required=False,
        )
        parser.add_argument(
            "-s",
            "--stub",
            help="fills in all missing translation with stub from EN",
            action="store_true",
            required=False,
        )
        self.args = parser.parse_args()
        self.args.program = sys.argv[0]
        if self.args.program[0] != "/":
            self.args.program = os.path.join(os.getcwd(), self.args.program)
        self.args.pdir = os.path.normpath(os.path.dirname(self.args.program))
        self.args.xousdir = os.path.normpath(os.path.dirname(self.args.pdir))
        self.args.program = os.path.basename(self.args.program)
        self.errfile = sys.stderr
        # Regex to match strings ending with ' *EN*' or any other language code (e.g., ' *FR*')
        self.temp_regex = re.compile(r"^.* \*[a-zA-Z]{2}\*$")
        self.mt_regex = re.compile(r"^.* \*MT\*$")

    def out(self, *objects):
        print(*objects)

    def err(self, *objects):
        print(*objects, file=self.errfile)

    def verr(self, *objects):
        if self.args.verbose:
            self.err(*objects)

    def get_languages(self):
        self.verr("-- get languages --")
        # services/root-keys/locales/i18n.json
        root_keys = os.path.join(
            self.args.xousdir, "services", "root-keys", "locales", "i18n.json"
        )
        if not os.path.exists(root_keys):
            self.err("cannot find: %s" % root_keys)
            return 1
        # read keys in "rootkeys.backup_key":
        essential = "rootkeys.backup_key"
        try:
            with open(root_keys, encoding='utf-8') as f:
                obj = json.load(f)
                self.languages = []
                if essential in obj:
                    for key in obj[essential].keys():
                        self.languages.append(key)
        except Exception as e:
            self.err(f"Error: {e}")
            return 1
        return 0

    def list_languages(self):
        if self.get_languages() != 0:
            return 1
        self.verr("-- list languages --")
        for lang in self.languages:
            self.out(lang)
        return 0

    def get_i18n_files(self):
        self.verr("-- get i18n files --")
        self.i18n_files = []
        topdirs = ["apps", "services", "libs"]
        # TODO: use glob to replicate exactly how rust looks for them.
        for top in topdirs:
            topdir = os.path.join(self.args.xousdir, top)
            if os.path.exists(topdir):
                if top == "apps":
                    manifest = os.path.join(topdir, "manifest.json")
                    if os.path.exists(manifest):
                        self.i18n_files.append(manifest[len(self.args.xousdir) + 1 :])
                for thing in os.listdir(topdir):
                    thingdir = os.path.join(topdir, thing)
                    if os.path.isdir(thingdir):
                        i18n_path = os.path.join(thingdir, "locales", "i18n.json")
                        if os.path.exists(i18n_path):
                            self.i18n_files.append(
                                i18n_path[len(self.args.xousdir) + 1 :]
                            )
        return 0

    def list_i18n_files(self):
        if self.get_i18n_files() != 0:
            return 1
        self.verr("-- list i18n files --")
        for pathname in self.i18n_files:
            self.out(pathname)
        return 0

    # keys for
    #   manifest APP, menu_name, appmenu.APP
    #   other    TAG
    def show_missing(self, i18n, is_manifest):
        i18n_path = os.path.join(self.args.xousdir, i18n)
        try:
            with open(i18n_path, encoding='utf-8') as f:
                obj = json.load(f)
        except Exception as e:
            self.err(f"Error: {e}")
            return 1

        jqpath = None
        translation = None
        for tag in obj.keys():
            if is_manifest:
                appmenu = "appmenu." + tag
                jqpath = tag + ".menu_name." + appmenu
                translation = obj[tag]["menu_name"][appmenu]
            else:
                jqpath = tag
                translation = obj[tag]
            for lang in self.languages:
                lang_path = jqpath + "." + lang
                status = "OK"
                if lang in translation:
                    t = translation[lang]
                    # to print translation
                    # print('%s\t%s\t%s' % (i18n, lang_path, t))
                    if t == "🔇":
                        status = "MISSING"
                    elif self.mt_regex.fullmatch(t):
                        status = "MACHINE_TRANSLATION"
                    elif self.temp_regex.fullmatch(t):
                        status = "TEMPORARY"
                else:
                    status = "ABSENT"
                if self.args.show_ok or status != "OK":
                    print("%s\t%s\t%s" % (i18n, lang_path, status))
        return 0

    def stub_missing(self, i18n, is_manifest):
        i18n_path = os.path.join(self.args.xousdir, i18n)
        try:
            with open(i18n_path, encoding='utf-8') as f:
                obj = json.load(f)
        except Exception as e:
            self.err(f"Error: {e}")
            return 1

        jqpath = None
        translation = None
        modified = False

        for tag in obj.keys():
            if is_manifest:
                appmenu = "appmenu." + tag
                jqpath = tag + ".menu_name." + appmenu
                translation = obj[tag]["menu_name"][appmenu]
            else:
                jqpath = tag
                translation = obj[tag]
            for lang in self.languages:
                lang_path = jqpath + "." + lang
                if not lang in translation:
                    if lang == "en-tts":
                        stub = translation['en']
                    else:
                        stub = f"{translation['en']} *EN*"
                    print(f"In {i18n}, at {lang_path} with en stub: {stub}")
                    translation[lang] = stub
                    modified = True
        if modified:
            # blow 'em away. That's what git is for after all.
            try:
                with open(i18n_path, 'w', encoding='utf-8') as f:
                    json.dump(obj, f, ensure_ascii=False, check_circular=False, sort_keys=True, indent=4)
            except Exception as e:
                self.err(f"Error: {e}")
                return 1

        return 0

    def fill_missing_with_en_stub(self):
        if self.get_languages() != 0:
            return 1
        if self.get_i18n_files() != 0:
            return 1
        self.verr("-- filling missing --")
        for i18n in self.i18n_files:
            is_manifest = os.path.basename(i18n) == "manifest.json"
            if self.stub_missing(i18n, is_manifest) != 0:
                return 1
        return 0

    def missing(self):
        if self.get_languages() != 0:
            return 1
        if self.get_i18n_files() != 0:
            return 1
        self.verr("-- missing --")
        for i18n in self.i18n_files:
            is_manifest = os.path.basename(i18n) == "manifest.json"
            if self.show_missing(i18n, is_manifest) != 0:
                return 1
        return 0

    # keys for
    #   manifest APP, menu_name, appmenu.APP
    #   other    TAG
    def add_new_lang(self, i18n, is_manifest):
        i18n_path = os.path.join(self.args.xousdir, i18n)
        i18n_path_orig = i18n_path + ".orig"
        if not os.path.exists(i18n_path_orig):
            os.rename(i18n_path, i18n_path_orig)
        self.verr('adding "%s" to %s' % (self.args.new_lang, i18n))

        try:
            with open(i18n_path_orig, encoding='utf-8') as orig_file:
                obj = json.load(orig_file)
        except Exception as e:
            self.err(f"Error: {e}")
            return 1

        translation = None
        for tag in obj.keys():
            if is_manifest:
                appmenu = "appmenu." + tag
                translation = obj[tag]["menu_name"][appmenu]
            else:
                translation = obj[tag]
            t_from = translation[self.args.from_lang]
            t_new = t_from
            if t_from != "🔇":
                t_new += self.from_hint
            translation[self.args.new_lang] = t_new

        try:
            with open(i18n_path, 'w', encoding='utf-8') as new_file:
                json.dump(
                    obj,
                    new_file,
                    ensure_ascii=False,
                    check_circular=False,
                    sort_keys=True,
                    indent=4,
                )
        except Exception as e:
            self.err(f"Error: {e}")
            return 1

        return 0

    def new_lang(self):
        if self.get_i18n_files() != 0:
            return 1
        self.from_hint = " *%s*" % self.args.from_lang.upper()
        self.verr(
            '-- adding new lang "%s" from "%s" by appending "%s" --'
            % (self.args.new_lang, self.args.from_lang, self.from_hint)
        )
        for i18n in self.i18n_files:
            is_manifest = os.path.basename(i18n) == "manifest.json"
            if self.add_new_lang(i18n, is_manifest) != 0:
                return 1
        return 0

    def run(self):
        rc = 0
        if self.args.verbose:
            self.err("verbose mode")
            self.err('xous dir:    "%s"' % self.args.xousdir)
            self.err('program dir: "%s"' % self.args.pdir)
            self.err('program:     "%s"' % self.args.program)
            self.err('show-ok:     "%s"' % self.args.show_ok)
        if self.args.list_languages:
            rc = self.list_languages()
        elif self.args.list_i18n_files:
            rc = self.list_i18n_files()
        elif self.args.missing:
            rc = self.missing()
        elif self.args.stub:
            rc = self.fill_missing_with_en_stub()
        elif self.args.new_lang:
            if not self.args.from_lang:
                self.err("error: you must specify both --from-lang en --new-lang fr")
                rc = 1
            else:
                self.get_languages()
                if not self.args.from_lang in self.languages:
                    self.err(
                        "error: --from-lang %s is not one of the existing langs: %s"
                        % (self.args.from_lang, self.languages)
                    )
                    rc = 1
                elif self.args.new_lang in self.languages:
                    self.err(
                        "error: --new-lang %s must not be one of the existing langs: %s"
                        % (self.args.from_lang, self.languages)
                    )
                    rc = 1
                elif not re.fullmatch("^[a-z-]{2,6}$", self.args.new_lang):
                    self.err(
                        "error: --new-lang %s is not valid lang" % self.args.new_lang
                    )
                    rc = 1
                else:
                    rc = self.new_lang()
        return rc


if __name__ == "__main__":
    sys.exit(Main().run())

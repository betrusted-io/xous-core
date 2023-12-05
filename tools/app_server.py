#! /usr/bin/env python3

import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import unquote

class XousAppServer(BaseHTTPRequestHandler):
    def __init__(self, profile, target, context_to_app, context_to_menus, *args):
        self.profile = profile
        self.target = target
        self.context_to_app = context_to_app
        self.context_to_menus = context_to_menus
        BaseHTTPRequestHandler.__init__(self, *args)

    def do_GET(self):
        # an empty path should return a list of apps
        path = self.path.split('/')[-1]
        if path == '':
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.end_headers()

            l = json.dumps(list(self.context_to_menus.items()))

            self.wfile.write(l.encode())
        else:
            name = self.context_to_app[unquote(path)]
            with open('target/{}/{}/{}'.format(self.target, self.profile, name), 'rb') as f:
                data = f.read()
                self.send_response(200)
                self.send_header('Content-Type', 'application/octet-stream')
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()

                self.wfile.write(data)

def main():
    parser = argparse.ArgumentParser(description="Xous App Server")
    parser.add_argument('apps', metavar='APP', nargs='+',
                        help='Specifies which apps to serve')
    parser.add_argument('-p', '--port',
                        help='Specifies the port that the server runs on',
                        required=True, type=int)
    parser.add_argument('--profile', default="release",
                        help="Either debug or release depending on how the apps were compiled. Defaults to release")
    parser.add_argument('--target', default="riscv32imac-unknown-xous-elf",
                        help="The target that the apps were compile to. Defaults to riscv32imac-unknown-xous-elf")
    args = parser.parse_args()

    # get the GAM names for each of the apps
    with open('apps/manifest.json', 'r') as f:
        manifest = json.loads(f.read())

    context_to_app = {}
    context_to_menus = {}

    for app in args.apps:
        context_name = manifest[app]['context_name']
        context_to_app[context_name] = app
        if 'submenu' in manifest[app]:
            context_to_menus[context_name] = manifest[app]['submenu']
        else:
            context_to_menus[context_name] = 0

    server = HTTPServer(('0.0.0.0', args.port), lambda *server_args: XousAppServer(args.profile, args.target, context_to_app, context_to_menus, *server_args))
    server.serve_forever()

if __name__ == "__main__":
    main()

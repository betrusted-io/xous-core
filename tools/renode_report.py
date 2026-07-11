#! /usr/bin/env python3
"""Render a self-contained HTML report from an on-target test console log.

The pddb-fs-tests / net-tests runners print one machine-parsable line per test
(`TEST <name> PASS|FAIL|XFAIL|XPASS [detail]`) and a totals line
(`FS-TESTS DONE:` / `NET-TESTS DONE: pass=.. fail=.. xfail=.. xpass=.. total=..`)
on the console UART. The robot suite that gates CI is a SINGLE robot test case
(boot + run + assert the sentinel stream), so its own report shows "1 test" and
pulls web fonts from Google. This produces a standalone report of the actual
per-test results instead: one HTML file, inline CSS, system fonts, no
JavaScript and no external requests, so it opens offline straight from the
downloaded artifact.

The same data also renders as GitHub-flavored Markdown (`--format md`), for
writing to `$GITHUB_STEP_SUMMARY` so the per-test results show directly on the
Actions run page (and, on pull_request runs, in the PR) without downloading the
artifact.

Usage: python3 tools/renode_report.py <console-log> [-o report.html]
       python3 tools/renode_report.py <console-log> --format md >> "$GITHUB_STEP_SUMMARY"
"""
import argparse
import html
import re
import sys

RE_TEST = re.compile(
    r'TEST (\S+) (PASS|FAIL|XFAIL|XPASS)(?:\s+(.*?))?\s*$')
RE_DONE = re.compile(
    r'(FS|NET)-TESTS DONE: pass=(\d+) fail=(\d+) xfail=(\d+) xpass=(\d+) total=(\d+)')
# Trailing source location the runner appends, e.g. " (services/.../main.rs:74)".
RE_LOC = re.compile(r'\s*\(services/[^)]*\)\s*$')

STATUS_ORDER = {'FAIL': 0, 'XPASS': 1, 'XFAIL': 2, 'PASS': 3}


def parse(lines):
    tests = []  # (name, status, detail), in first-seen order, de-duped
    seen = set()
    done = None
    suite = 'std'
    for raw in lines:
        line = raw.rstrip('\n')
        m = RE_DONE.search(line)
        if m:
            suite = {'FS': 'std::fs', 'NET': 'std::net'}.get(m.group(1), m.group(1))
            done = tuple(int(x) for x in m.groups()[1:])
            continue
        m = RE_TEST.search(line)
        if not m:
            continue
        name, status, detail = m.group(1), m.group(2), (m.group(3) or '')
        detail = RE_LOC.sub('', detail).strip()
        if name in seen:
            continue
        seen.add(name)
        tests.append((name, status, detail))
    return suite, tests, done


def render(suite, tests, done):
    counts = {'PASS': 0, 'FAIL': 0, 'XFAIL': 0, 'XPASS': 0}
    for _, s, _ in tests:
        counts[s] = counts.get(s, 0) + 1
    total = len(tests)
    green = counts['FAIL'] == 0 and counts['XPASS'] == 0
    if done is not None:
        d_pass, d_fail, d_xfail, d_xpass, d_total = done
        green = d_fail == 0 and d_xpass == 0
    banner = 'PASS' if green else 'FAIL'

    # group by theme (text before "::")
    themes = {}
    for name, status, detail in tests:
        theme = name.split('::', 1)[0] if '::' in name else '(ungrouped)'
        themes.setdefault(theme, []).append((name, status, detail))

    def esc(s):
        return html.escape(s, quote=True)

    rows = []
    for theme in themes:
        items = sorted(themes[theme], key=lambda t: STATUS_ORDER.get(t[1], 9))
        rows.append('<tr class="theme"><td colspan="3">{} '
                    '<span class="n">({})</span></td></tr>'.format(esc(theme), len(items)))
        for name, status, detail in items:
            short = name.split('::', 1)[1] if '::' in name else name
            rows.append(
                '<tr><td class="name">{}</td>'
                '<td><span class="badge {s}">{s}</span></td>'
                '<td class="detail">{d}</td></tr>'.format(
                    esc(short), s=status, d=esc(detail)))
    table = '\n'.join(rows)

    summary = ('pass={PASS} fail={FAIL} xfail={XFAIL} xpass={XPASS} '
               'total={t}'.format(t=total, **counts))
    note = ('' if green else
            '<p class="warn">This run is RED: a FAIL is a real defect; an '
            'XPASS means a known-bad behavior started passing — update the '
            'XFAIL registry.</p>')

    return TEMPLATE.format(
        suite=esc(suite), banner=banner, banner_cls=banner.lower(),
        summary=esc(summary), pass_=counts['PASS'], fail=counts['FAIL'],
        xfail=counts['XFAIL'], xpass=counts['XPASS'], total=total,
        table=table, note=note)


# Fully self-contained: system font stacks only (no @import / no web fonts),
# inline CSS, no JavaScript, no external requests -- opens offline.
TEMPLATE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{suite} on-target report</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ font-family: system-ui, -apple-system, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
         margin: 0; padding: 1.5rem; line-height: 1.4;
         color: #1a1a1a; background: #fafafa; }}
  @media (prefers-color-scheme: dark) {{
    body {{ color: #e6e6e6; background: #161616; }}
    tr.theme td {{ background: #222 !important; }}
    td, th {{ border-color: #333 !important; }}
    .card {{ background: #1f1f1f !important; border-color: #333 !important; }}
  }}
  h1 {{ font-size: 1.3rem; margin: 0 0 .25rem; }}
  .sub {{ color: #888; margin: 0 0 1rem; }}
  .banner {{ display: inline-block; font-weight: 700; letter-spacing: .05em;
             padding: .35rem .9rem; border-radius: .4rem; color: #fff; }}
  .banner.pass {{ background: #1a7f37; }}
  .banner.fail {{ background: #c1272d; }}
  .cards {{ display: flex; flex-wrap: wrap; gap: .6rem; margin: 1rem 0 1.2rem; }}
  .card {{ border: 1px solid #ddd; border-radius: .5rem; padding: .5rem .8rem;
           min-width: 5rem; background: #fff; }}
  .card .v {{ font-size: 1.5rem; font-weight: 700; }}
  .card .k {{ font-size: .75rem; text-transform: uppercase; color: #888; letter-spacing: .04em; }}
  table {{ border-collapse: collapse; width: 100%; }}
  td, th {{ border-bottom: 1px solid #e2e2e2; padding: .3rem .5rem; text-align: left;
            vertical-align: top; font-size: .9rem; }}
  tr.theme td {{ background: #f0f0f0; font-weight: 600; padding-top: .6rem; }}
  tr.theme .n {{ color: #999; font-weight: 400; }}
  td.name {{ font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace; }}
  td.detail {{ color: #888; font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
               font-size: .82rem; word-break: break-word; }}
  .badge {{ font-size: .72rem; font-weight: 700; padding: .1rem .45rem; border-radius: .3rem;
            color: #fff; }}
  .badge.PASS {{ background: #1a7f37; }}
  .badge.XFAIL {{ background: #9a6700; }}
  .badge.FAIL {{ background: #c1272d; }}
  .badge.XPASS {{ background: #bc4c00; }}
  .warn {{ color: #c1272d; font-weight: 600; }}
  .legend {{ color: #888; font-size: .8rem; margin-top: 1.2rem; }}
</style></head><body>
<h1>{suite} &mdash; on-target Renode CI <span class="banner {banner_cls}">{banner}</span></h1>
<p class="sub">{summary}</p>
{note}
<div class="cards">
  <div class="card"><div class="v">{pass_}</div><div class="k">pass</div></div>
  <div class="card"><div class="v">{fail}</div><div class="k">fail</div></div>
  <div class="card"><div class="v">{xfail}</div><div class="k">xfail</div></div>
  <div class="card"><div class="v">{xpass}</div><div class="k">xpass</div></div>
  <div class="card"><div class="v">{total}</div><div class="k">total</div></div>
</div>
<table>{table}</table>
<p class="legend">XFAIL = a known bug, pinned so the suite stays green while it
stays visible (the detail column names the bug id). XPASS = a pinned bug
apparently fixed &mdash; update the registry. Generated offline from the console
sentinel stream; no external resources.</p>
</body></html>
"""


BADGE_MD = {'PASS': '✅ PASS', 'XFAIL': '⚠️ XFAIL', 'FAIL': '❌ FAIL', 'XPASS': '❗ XPASS'}


def render_markdown(suite, tests, done):
    """GitHub-flavored Markdown for $GITHUB_STEP_SUMMARY / a PR comment."""
    counts = {'PASS': 0, 'FAIL': 0, 'XFAIL': 0, 'XPASS': 0}
    for _, s, _ in tests:
        counts[s] = counts.get(s, 0) + 1
    total = len(tests)
    green = counts['FAIL'] == 0 and counts['XPASS'] == 0
    if done is not None:
        green = done[1] == 0 and done[3] == 0
    head = '✅ PASS' if green else '❌ FAIL'

    out = []
    out.append('## {} — on-target Renode CI &nbsp; {}'.format(suite, head))
    out.append('')
    out.append('`pass={PASS}  fail={FAIL}  xfail={XFAIL}  xpass={XPASS}  '
               'total={t}`'.format(t=total, **counts))
    out.append('')
    # Surface anything red up front.
    red = [(n, s, d) for n, s, d in tests if s in ('FAIL', 'XPASS')]
    if red:
        out.append('### Needs attention')
        out.append('| test | result | detail |')
        out.append('|---|---|---|')
        for n, s, d in red:
            out.append('| `{}` | {} | {} |'.format(n, BADGE_MD[s], d.replace('|', '\\|')))
        out.append('')

    # Full per-test list, grouped by theme, collapsed by default.
    themes = {}
    for n, s, d in tests:
        themes.setdefault(n.split('::', 1)[0] if '::' in n else '(ungrouped)', []).append((n, s, d))
    out.append('<details><summary>All {} tests</summary>'.format(total))
    out.append('')
    for theme in themes:
        items = sorted(themes[theme], key=lambda t: STATUS_ORDER.get(t[1], 9))
        out.append('**{}** ({})'.format(theme, len(items)))
        out.append('')
        out.append('| test | result | detail |')
        out.append('|---|---|---|')
        for n, s, d in items:
            short = n.split('::', 1)[1] if '::' in n else n
            out.append('| `{}` | {} | {} |'.format(short, BADGE_MD[s], d.replace('|', '\\|')))
        out.append('')
    out.append('</details>')
    out.append('')
    return '\n'.join(out)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('console_log')
    ap.add_argument('-o', '--output', default='-')
    ap.add_argument('--format', choices=['html', 'md'], default='html')
    args = ap.parse_args()
    with open(args.console_log, encoding='utf-8', errors='replace') as f:
        suite, tests, done = parse(f)
    if not tests:
        sys.stderr.write('renode_report: no TEST sentinels found in {}\n'.format(args.console_log))
        return 2
    out = render_markdown(suite, tests, done) if args.format == 'md' else render(suite, tests, done)
    if args.output == '-':
        sys.stdout.write(out)
    else:
        with open(args.output, 'w', encoding='utf-8') as f:
            f.write(out)
    return 0


if __name__ == '__main__':
    sys.exit(main())

#!/usr/bin/env python3
"""Render a synthetic CLI transcript for visual QA; runs no agent or shell commands."""
import shutil
import sys

GREEN = '\033[38;2;82;157;91m'
MUTED = '\033[38;2;145;145;145m'
BOLD = '\033[1m'
RESET = '\033[0m'

lines = [
    '', '', '', '',
    '  Pricing table copy is in and the FAQ answers the three main questions.',
    '',
    f'{MUTED}* Worked for 12m 04s · done 9:41 AM{RESET}',
    '',
    '› Ship the new landing hero today. Split it up: copy, OG image, and video.',
    '',
    f'{GREEN}●{RESET} {BOLD}Read{RESET}(src/components/Hero.tsx, src/styles/hero.css)',
    f'{MUTED}  └ Read 2 files (412 lines){RESET}',
    '',
    f'{GREEN}●{RESET} {BOLD}Task{RESET}(spawn 3 background agents)',
    f'{MUTED}  └ hero: copy variants · hero: og image · hero: video{RESET}',
    '',
    f'{GREEN}●{RESET} {BOLD}Write{RESET}(src/components/Hero.tsx)',
    f'{MUTED}  └ Wrote 86 lines{RESET}',
    '',
    f'{GREEN}●{RESET} {BOLD}Bash{RESET}(npm run build)',
    f'{MUTED}  └ ✓ Compiled in 4.2s · 0 errors · 0 warnings{RESET}',
    '',
    f'{MUTED}* Waiting for 3 background agents to finish{RESET}',
    '',
    f'{GREEN}●{RESET} {BOLD}Agent "hero: copy variants" finished{RESET} {MUTED}· 3m 41s{RESET}',
    '  Wrote 6 headline variants for the hero. The shortest one is ready.',
    '',
    f'{MUTED}* Waiting for 2 background agents to finish{RESET}',
]

if not sys.stdout.isatty():
    raise SystemExit('Run this preview inside Torch.')
try:
    sys.stdout.write('\033[?1049h\033[2J\033[H\033[?25l\033[?7l')
    height = shutil.get_terminal_size().lines
    for line in lines[:max(0, height - 3)]:
        sys.stdout.write(line + '\r\n')
    sys.stdout.write(f'\033[{height - 1};1H\033[2K{MUTED}Visual test fixture · synthetic transcript · press Enter to exit{RESET}')
    sys.stdout.flush()
    input()
finally:
    sys.stdout.write('\033[?7h\033[?25h\033[?1049l')
    sys.stdout.flush()

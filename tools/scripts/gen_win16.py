"""Generate PMWIN's 16-bit entry points (the OS/2 1.x API: Win16*, Gpi16*).

16-bit programs import PMWIN and PMGPI ordinals below 700 (and some
PMMERGE ones), `_pascal far` functions with 16-bit integers and 16:16
pointers. PMWIN implements them all (src/win16.rs): an entry whose
arguments only need widening and pointer conversion calls PMWIN's 32-bit
export through a generic thunk driven by its argument types (from
userland/pmwin/ibm-16bit.txt); the others have handlers in win16.rs
(CUSTOM below). The 16-bit GPI entries are PMWIN ordinals 4000 + the
PMGPI ordinal, as the 32-bit ones are.

Writes userland/pmwin/src/win16_table.rs (the win16! list),
userland/pmwin/exports.extra (PMWIN's 16-bit exports), and
userland/pmgpi/exports.extra (PMGPI forwarders to them). gen_pm_exports.py
appends the exports.extra files to the exports.def files; run it after
this. gen_pmmerge_forwards.py makes PMMERGE's 16-bit ordinals forwarders.

usage: python3 -I gen_win16.py [USERLAND_DIR]
"""
import os, re, sys

ul = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), '..', '..', 'userland')
win = os.path.join(ul, 'pmwin')

# Argument types of entries IBM's table does not give (own thunks or
# 16-bit code there).
SPEC = {
    ('PMWIN', 6): 'lppliiiillwpp',   # WinCreateWindow
    ('PMWIN', 58): 'li',             # WinCreateMsgQueue
    ('PMWIN', 140): 'llppplwwp',     # WinCreateStdWindow
    ('PMWIN', 159): 'lp',            # WinSetRectEmpty
    ('PMWIN', 160): 'lpiiii',        # WinSetRect
    ('PMWIN', 161): 'lpp',           # WinCopyRect
    ('PMWIN', 162): 'lp',            # WinIsRectEmpty
    ('PMWIN', 164): 'lpp',           # WinPtInRect
    ('PMWIN', 165): 'lpii',          # WinOffsetRect
    ('PMWIN', 166): 'lpii',          # WinInflateRect
    ('PMWIN', 211): 'li',            # WinGetKeyState
    ('PMWIN', 235): 'lp',            # WinAddAtom
    ('PMWIN', 236): 'lp',            # WinFindAtom
}

# Entries with a handler in win16.rs: handler, argument types.
CUSTOM = {
    ('PMWIN', 3): ('register_class', 'lpplw'),
    ('PMWIN', 5): ('query_class_info', 'lpp'),
    ('PMWIN', 9): ('set_mult_window_pos', 'lpw'),
    ('PMWIN', 10): ('query_window_pos', 'lp'),
    ('PMWIN', 27): ('query_window_process', 'lpp'),
    ('PMWIN', 30): ('set_window_ulong', 'lil'),
    ('PMWIN', 31): ('query_window_ulong', 'li'),
    ('PMWIN', 37): ('subclass_window', 'll'),
    ('PMWIN', 61): ('send_msg', 'lwll'),
    ('PMWIN', 63): ('broadcast_msg', 'lwllw'),
    ('PMWIN', 65): ('get_msg', 'lplww'),
    ('PMWIN', 66): ('peek_msg', 'lplwww'),
    ('PMWIN', 68): ('dispatch_msg', 'lp'),
    ('PMWIN', 69): ('post_msg', 'lwll'),
    ('PMWIN', 70): ('post_queue_msg', 'lwll'),
    ('PMWIN', 101): ('query_cursor_info', 'lp'),
    ('PMWIN', 105): ('translate_accel', 'lllp'),
    ('PMWIN', 109): ('set_hook', 'llilw'),
    ('PMWIN', 110): ('release_hook', 'llilw'),
    ('PMWIN', 116): ('set_clipbrd_data', 'llww'),
    ('PMWIN', 117): ('query_clipbrd_data', 'lw'),
    ('PMWIN', 119): ('query_clipbrd_fmt_info', 'lwp'),
    ('PMWIN', 122): ('load_dlg', 'lllwwl'),
    ('PMWIN', 123): ('create_dlg', 'lllpl'),
    ('PMWIN', 125): ('dlg_box', 'lllwwl'),
    ('PMWIN', 127): ('send_dlg_item_msg', 'lwwll'),
    ('PMWIN', 154): ('track_rect', 'llp'),
    ('PMWIN', 178): ('def_window_proc', 'lwll'),
    ('PMWIN', 179): ('def_dlg_proc', 'lwll'),
    ('PMWIN', 217): ('query_cp_list', 'lwp'),
    ('PMWIN', 223): ('next_char', 'lwwp'),
    ('PMWIN', 224): ('prev_char', 'lwwpp'),
    ('PMWIN', 244): ('get_error_info', 'l'),
    ('PMWIN', 245): ('free_error_info', 'p'),
    ('PMWIN', 269): ('query_window_ulong', 'li'),
    ('PMWIN', 270): ('set_window_ulong', 'lil'),
    ('PMWIN', 288): ('call_help_hook', 'iiip'),
    ('PMWIN', 310): ('build_ptr_handle', 'p'),
    ('PMWIN', 317): ('query_window_model', 'l'),
    ('PMGPI', 1): ('dev_open_dc', 'llplpl'),
}

# Entries that are 16-bit code in win16.rs (exported as they are).
NATIVE = {('PMWIN', 248), ('PMWIN', 249)}  # WinCatch, WinThrow


def size(spec):
    return sum(4 if c in 'lp' else 2 for c in spec)


exports32 = {}
for l in open(os.path.join(win, 'exports.def')):
    m = re.match(r'(\w+)=__a_\w+\s*@(\d+)', l.strip())
    if m:
        exports32[m.group(1).upper()] = m.group(1)

api, custom, extra, gpi = [], [], [], []
for l in open(os.path.join(win, 'ibm-16bit.txt')):
    if l.startswith(';') or not l.strip():
        continue
    mod, o, name, spec = l.split()
    o = int(o)
    if mod not in ('PMWIN', 'PMGPI'):
        continue
    idx = o if mod == 'PMWIN' else 4000 + o
    if (mod, o) in NATIVE:
        pass
    elif (mod, o) in CUSTOM:
        h, spec = CUSTOM[(mod, o)]
        custom.append((idx, name, size(spec), h))
    else:
        spec = SPEC.get((mod, o), spec)
        if spec == '-':
            spec = ''
        target = exports32.get(name.replace('16', '', 1))
        if not re.fullmatch(r'[lpwi]*', spec) or target is None:
            continue
        api.append((idx, name, size(spec), spec, target))
    # Exported under IBM's names for them (WINCREATEWINDOW): 16-bit
    # programs look names up in upper case, which then finds these before
    # the 32-bit entries (WinCreateWindow).
    export = name.replace('16', '', 1)
    extra.append(f'{export}={name} @{idx}')
    if mod == 'PMGPI':
        gpi.append(f'{export}=PMWIN.{idx} @{o}')

rs = ['// Generated by tools/scripts/gen_win16.py: PMWIN\'s 16-bit entry points', '// (ordinal name argument-size types target, or ordinal name size handler).', '',
      'win16! {', '    api {']
rs += [f'        {i} {n} {s} "{sp}" __a_{t};' for i, n, s, sp, t in sorted(api)]
rs += ['    }', '    custom {']
rs += [f'        {i} {n} {s} {h};' for i, n, s, h in sorted(custom)]
rs += ['    }', '}']
open(os.path.join(win, 'src', 'win16_table.rs'), 'w').write('\n'.join(rs) + '\n')
open(os.path.join(win, 'exports.extra'), 'w').write('; Generated by tools/scripts/gen_win16.py: 16-bit entry points\n' + '\n'.join(sorted(extra, key=lambda x: int(x.rsplit('@', 1)[1]))) + '\n')
open(os.path.join(ul, 'pmgpi', 'exports.extra'), 'w').write('; Generated by tools/scripts/gen_win16.py: 16-bit entry points (in PMWIN)\n' + '\n'.join(gpi) + '\n')
print(f'{len(api)} table entries, {len(custom)} handlers, {len(gpi)} PMGPI forwarders')

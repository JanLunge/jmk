#!/usr/bin/env python3
"""Writes svalboard.jmk.json: the Svalboard (V2b PFET controller board) as a JMK keyboard file.

Wiring and key order follow the Svalboard's own firmware (github.com/svalboard/vial-qmk,
keyboards/svalboard: info.json, config.h, matrix.c and keymaps/vial/keymap.c).
"""
import json, pathlib

ROWS = [8, 7, 6, 5, 4]          # thumb, index, middle, ring, pinky; low powers that cluster
COLS = [14, 13, 12, 11, 10, 9]
# Matrix column of each key, in the order the stock keymap lists them.
FINGER = {'C': 2, 'N': 3, 'E': 1, 'S': 0, 'W': 4, 'DD': 5}
THUMB = {'Down': 2, 'Pad': 3, 'Up': 4, 'Nail': 1, 'Knuckle': 0, 'DD': 5}
PER_HALF = len(ROWS) * len(COLS)

def pos(half, row, col):
    return half * PER_HALF + row * len(COLS) + col

# --- Keycodes ----------------------------------------------------------------
HID = {**{chr(ord('A') + i): 4 + i for i in range(26)}, **{str(i): 29 + i for i in range(1, 10)}, '0': 39,
       'ENTER': 40, 'ESC': 41, 'BSPC': 42, 'TAB': 43, 'SPACE': 44, 'MINUS': 45, 'EQUAL': 46, 'LBRC': 47,
       'RBRC': 48, 'BSLS': 49, 'SCLN': 51, 'QUOT': 52, 'GRV': 53, 'COMM': 54, 'DOT': 55, 'SLSH': 56,
       'CAPS': 57, 'INS': 73, 'HOME': 74, 'PGUP': 75, 'DEL': 76, 'END': 77, 'PGDN': 78, 'RIGHT': 79,
       'LEFT': 80, 'DOWN': 81, 'UP': 82, 'LCTL': 0xE0, 'LSFT': 0xE1, 'LALT': 0xE2, 'LGUI': 0xE3,
       **{f'F{i}': 57 + i for i in range(1, 13)}, **{f'F{i}': 91 + i for i in range(13, 25)}}
SHIFT, GUI = 2, 8
NAVNAS, FUNC = 1, 2

def key(name): return {'Basic': {'Key': HID[name]}}
def shifted(name): return {'Basic': {'ModKey': {'mods': SHIFT, 'key': HID[name]}}}
def held(hold, tap): return {'HoldTap': {'hold': hold, 'tap': {'Key': HID[tap]}, 'profile': 0}}
NO, TR = {'Basic': 'None'}, 'Transparent'
def mo(layer): return {'Basic': {'MomentaryLayer': layer}}

def parse(text):
    """A few stock QMK names, enough for the default keymap."""
    if text == 'NO': return NO
    if text == 'TR': return TR
    if text.startswith('S('): return shifted(text[2:-1])
    return key(text)

# Finger clusters: Center North East South West Double, as in keymap.c.
FINGERS = {
    0: {  # NORMAL
        'R1': 'J U QUOT M H NO', 'R2': 'K I S(SCLN) COMM Y NO', 'R3': 'L O NO DOT N NO', 'R4': 'SCLN P BSLS SLSH RBRC NO',
        'L1': 'F R G V S(QUOT) NO', 'L2': 'D E T C GRV NO', 'L3': 'S W B X ESC NO', 'L4': 'A Q LBRC Z DEL NO'},
    NAVNAS: {
        'R1': '7 S(7) S(6) LEFT 6 NO', 'R2': '8 S(8) NO UP S(MINUS) NO', 'R3': '9 S(9) NO DOWN INS NO', 'R4': '0 S(0) NO RIGHT S(GRV) NO',
        'L1': '4 S(4) 5 END S(5) NO', 'L2': '3 S(3) MINUS PGDN S(EQUAL) NO', 'L3': '2 S(2) DOT PGUP TR NO', 'L4': '1 S(1) EQUAL HOME TR NO'},
    FUNC: {
        'R1': 'F7 NO F16 F17 F6 NO', 'R2': 'F8 NO NO F18 NO NO', 'R3': 'F9 NO NO F19 NO NO', 'R4': 'F10 NO NO F20 NO NO',
        'L1': 'F4 F24 F5 F14 F15 NO', 'L2': 'F3 F23 F10 F13 NO NO', 'L3': 'F2 F22 NO F12 NO NO', 'L4': 'F1 F21 NO F11 NO NO'},
}
# Thumbs: Down Pad Up Nail Knuckle DoubleDown.
THUMBS = {
    0: {'RT': [mo(NAVNAS), key('SPACE'), NO, key('BSPC'), key('LALT'), mo(FUNC)],
        'LT': [key('LSFT'), held({'MomentaryLayer': NAVNAS}, 'ENTER'), NO, held({'Mods': GUI}, 'TAB'), key('LCTL'), key('CAPS')]},
    NAVNAS: {'RT': [TR] * 6, 'LT': [TR] * 6},
    FUNC: {'RT': [TR] * 6, 'LT': [TR] * 6},
}

def cluster(name):
    """(half, row) of a cluster name: L1 is the left index finger, R4 the right pinky, LT the left thumb."""
    half = 0 if name[0] == 'L' else 1
    return half, 0 if name[1] == 'T' else int(name[1])

layers = []
for layer in (0, NAVNAS, FUNC):
    actions = [TR if layer else NO] * (2 * PER_HALF)
    for name, keys in FINGERS[layer].items():
        half, row = cluster(name)
        for direction, text in zip(FINGER, keys.split()):
            actions[pos(half, row, FINGER[direction])] = parse(text)
    for name, keys in THUMBS[layer].items():
        half, row = cluster(name)
        for spot, action in zip(THUMB, keys):
            actions[pos(half, row, THUMB[spot])] = action
    layers.append(actions)

# --- Layout ------------------------------------------------------------------
U = 100
layout = [{'x': 0, 'y': 0, 'w': 0, 'h': 0, 'r': 0} for _ in range(2 * PER_HALF)]
def place(p, x, y, w, h): layout[p] = {'x': round(x * U), 'y': round(y * U), 'w': round(w * U), 'h': round(h * U), 'r': 0}

# A finger cluster as a cross around its centre key. Only the thumbs have a double-down key:
# the finger clusters' DD column reads whatever the thumb's does, so it stays off the picture.
STAGGER = {1: 1, 2: 0.5, 3: 1, 4: 1.5}   # index, middle, ring, pinky
LEFT_X = {4: 0, 3: 2.25, 2: 4.5, 1: 6.75}
RIGHT_X = {1: 11.75, 2: 14, 3: 16.25, 4: 18.5}
for half, xs in ((0, LEFT_X), (1, RIGHT_X)):
    for row, x in xs.items():
        y = STAGGER[row]
        place(pos(half, row, FINGER['W']), x, y + 0.5, 0.5, 1)
        place(pos(half, row, FINGER['N']), x + 0.5, y, 1, 0.5)
        place(pos(half, row, FINGER['C']), x + 0.5, y + 0.5, 1, 1)
        place(pos(half, row, FINGER['E']), x + 1.5, y + 0.5, 0.5, 1)
        place(pos(half, row, FINGER['S']), x + 0.5, y + 1.5, 1, 0.5)

# Thumbs, after the stock firmware's picture: the outer keys face the other hand.
def thumb(half, base, mirror):
    keys = {'Down': (1, 0.5, 1, 2), 'DD': (1, 2.5, 1, 0.5), 'Pad': (2, 0, 0.5, 1.5), 'Up': (2.5, 0, 0.5, 1),
            'Nail': (0.5, 0, 0.5, 1.5), 'Knuckle': (0.5, 1.5, 0.5, 1.5)}
    for spot, (dx, dy, w, h) in keys.items():
        x = base + (3 - dx - w if mirror else dx)
        place(pos(half, 0, THUMB[spot]), x, 4.25 + dy, w, h)
thumb(0, 6.5, True)
thumb(1, 10.75, False)

# --- Letter rows -------------------------------------------------------------
base = layers[0]
def where(name): return next(p for p, a in enumerate(base) if a == key(name))
letter_rows = [[where(c) for c in 'QWERTYUIOP'], [where(c) for c in 'ASDFGHJKL'] + [where('SCLN')],
               [where(c) for c in 'ZXCVBNM'] + [where('COMM'), where('DOT'), where('SLSH')]]

# --- Hardware ----------------------------------------------------------------
def hardware(half):
    return {
        'matrix': {'Matrix': {'rows': ROWS, 'cols': COLS, 'diodes': 'Col2Row'}},
        'scan': {
            # The centre key of each finger cluster reads closed at rest. Some thumb clusters read
            # their Down key that way too (THUMB_DOWN_ACTIVE_DARK in the stock firmware): add
            # position 2 then, or press "Treat the keys held now as resting" in POG.
            'inverted': [pos(0, row, FINGER['C']) for row in (1, 2, 3, 4)],
            # The stock firmware waits 90 us after switching a cluster on and off.
            'settle_us': 90,
            # A pull-up cannot switch a cluster's power off.
            'drive_idle_high': True,
        },
        'reserved_pins': [],
        'debounce_ms': 5,
        'encoders': [],
        # The split cable runs straight through, GP0 to GP0 and GP1 to GP1: the stock firmware
        # sends on GP0 from the left and on GP1 from the right. GP1 has no UART transmitter,
        # so the halves take turns on the GP0 wire.
        'split': {'Half': {'link': {'Wire': {'pin': 0}}, 'offset': half * PER_HALF}},
    }

definition = {
    'schema': 1,
    'name': 'Svalboard',
    'controller': 'generic',
    'mcu': 'Rp2040',
    'halves': [{'hardware': hardware(0)}, {'hardware': hardware(1)}],
    'layout': layout,
    'keymap': {
        'layers': layers,
        'profiles': [{'flavor': 'Balanced', 'tapping_term_ms': 200, 'require_prior_idle_ms': 0}] * 4,
        'combos': [],
        'macros': [[] for _ in range(16)],
    },
    'letterRows': letter_rows,
}
out = pathlib.Path(__file__).with_name('svalboard.jmk.json')
out.write_text(json.dumps(definition, indent=1) + '\n')
print('wrote', out)

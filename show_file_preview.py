#!/usr/bin/env python3
# show_file_preview.py
# Usage: python show_file_preview.py state_test_phase2.json

import sys
import binascii

def hexdump_preview(b):
    # show printable bytes and hex for first/last parts
    def make_part(part):
        printable = ''.join((chr(c) if 32 <= c <= 126 else '.') for c in part)
        hexs = binascii.hexlify(part).decode('ascii')
        return printable, hexs

    n = len(b)
    head = b[:200]
    tail = b[-200:] if n > 200 else b
    ph, hh = make_part(head)
    pt, ht = make_part(tail)
    return n, ph, hh, pt, ht

def main():
    if len(sys.argv) < 2:
        print("Usage: python show_file_preview.py <filename>")
        sys.exit(1)
    path = sys.argv[1]
    try:
        with open(path, "rb") as f:
            data = f.read()
    except Exception as e:
        print("ERROR reading file:", e)
        sys.exit(1)

    n, ph, hh, pt, ht = hexdump_preview(data)
    print("LEN:", n)
    print("--- HEAD (printable) ---")
    print(ph)
    print("--- HEAD (hex) ---")
    print(hh)
    print("--- TAIL (printable) ---")
    print(pt)
    print("--- TAIL (hex) ---")
    print(ht)

if __name__ == "__main__":
    main()
#!/usr/bin/env python3
"""Babel segment files: a .docx as segments to translate, and a check of a
translated file. Standard library only.

  babel_segments.py extract DOCUMENT.docx [--lang sv-SE] [-o OUT.md]
  babel_segments.py check TRANSLATED.md

The walk over a paragraph mirrors Babel's (crates/babel-editor/src/segments.rs):
keep the two alike, or Babel won't find the segments.
"""

import argparse
import posixpath
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

HEADER = (
    "---\nbabel-segments: 1\ndocument: {document}\ntarget-language: {lang}\n---\n\n"
    "<!-- One segment per paragraph of the original, in order: its number, the original quoted with \">\", "
    "and the translation on the lines below it. Keep {{1}}, {{2}}… (footnote marks, pictures, fields) and {{tab}} "
    "where they belong in the translation; **bold** and *italic* as in the original; a new line is a line break "
    "within the paragraph. -->\n"
)

OBJECTS = {"sym", "drawing", "pict", "object", "footnoteReference", "endnoteReference", "footnoteRef", "endnoteRef", "AlternateContent"}


def local(el):
    tag = el.tag
    return tag.rsplit("}", 1)[-1] if isinstance(tag, str) else ""


def attr(el, name):
    for k, v in el.attrib.items():
        if k == name or k.endswith("}" + name):
            return v
    return None


def is_hyperlink(instr):
    s = instr.lstrip()
    if s[:9].upper() != "HYPERLINK":
        return False
    rest = s[9:]
    if rest[:1] and (rest[0].isalnum() or rest[0] == "_"):
        return False
    return True


class Field:
    def __init__(self):
        self.instr = ""
        self.separated = False
        self.link = False


def hidden(fields):
    return any(f.separated and not f.link for f in fields)


def flag(rpr, name):
    if rpr is None:
        return False
    for e in rpr:
        if local(e) == name:
            return attr(e, "val") not in ("0", "false", "off")
    return False


def walk(p):
    """Items of paragraph p: ('t', text, b, i, link), ('tab',), ('br',), ('obj',)."""
    out, fields = [], []

    def run(r, link):
        kids = list(r)
        rpr = next((k for k in kids if local(k) == "rPr"), None)
        b, i = flag(rpr, "b"), flag(rpr, "i")
        for k in kids:
            name = local(k)
            l = link or any(f.separated and f.link for f in fields)
            if name == "fldChar":
                kind = attr(k, "fldCharType")
                if kind == "begin":
                    fields.append(Field())
                elif kind == "separate":
                    was = hidden(fields)
                    if fields:
                        f = fields[-1]
                        f.separated = True
                        f.link = is_hyperlink(f.instr)
                        if not f.link and not was:
                            out.append(("obj",))
                elif kind == "end":
                    if fields:
                        fields.pop()
            elif name == "instrText":
                if fields and not fields[-1].separated:
                    fields[-1].instr += "".join(k.itertext())
            elif hidden(fields):
                pass
            elif name == "t":
                out.append(("t", "".join(k.itertext()), b, i, l))
            elif name in ("tab", "ptab"):
                out.append(("tab",))
            elif name in ("br", "cr"):
                if (attr(k, "type") or "textWrapping") == "textWrapping":
                    out.append(("br",))
            elif name == "noBreakHyphen":
                out.append(("t", "‑", b, i, l))
            elif name in OBJECTS:
                out.append(("obj",))

    def visit(el, link):
        for c in el:
            name = local(c)
            if name in ("p", "pPr", "rPr", "Fallback", "del", "moveFrom"):
                continue
            if name == "hyperlink":
                visit(c, True)
            elif name == "fldSimple":
                if is_hyperlink(attr(c, "instr") or ""):
                    visit(c, True)
                elif not hidden(fields):
                    out.append(("obj",))
            elif name == "r":
                run(c, link)
            else:
                visit(c, link)

    visit(p, False)
    return out


def translatable(items):
    return any(it[0] == "t" and any(ch.isalpha() for ch in it[1]) for it in items)


def escape(s):
    return "".join("\\" + c if c in "\\*{[]" else c for c in s)


def span(text, b, i):
    mark = "***" if b and i else "**" if b else "*" if i else ""
    lines = []
    for line in text.split("\n"):
        core = line.strip()
        if not mark or not core:
            lines.append(escape(line))
            continue
        lead = line[: len(line) - len(line.lstrip())]
        trail = line[len(line.rstrip()):]
        lines.append(f"{escape(lead)}{mark}{escape(core)}{mark}{escape(trail)}")
    return "\n".join(lines)


def markup(items):
    s, objects, k, in_link = [], 0, 0, False
    while k < len(items):
        it = items[k]
        if it[0] == "t":
            b, i, l, text = it[2], it[3], it[4], ""
            if l != in_link:
                s.append("[" if l else "]")
                in_link = l
            while k < len(items) and items[k][0] == "t" and (items[k][2], items[k][3], items[k][4]) == (b, i, l):
                text += items[k][1]
                k += 1
            s.append(span(text, b, i))
            continue
        if it[0] == "tab":
            s.append("{tab}")
        elif it[0] == "br":
            s.append("\n")
        else:
            objects += 1
            s.append("{%d}" % objects)
        k += 1
    if in_link:
        s.append("]")
    joined = "".join(s)
    return "\n".join("\\" + l if l.startswith(("#", ">")) else l for l in joined.split("\n"))


# Python's str.strip and Rust's str::trim agree on what whitespace is for the
# text Word holds; isalpha and char::is_alphabetic differ only in rare scripts.


def relationships(xml):
    root = ET.fromstring(xml)
    return [(attr(r, "Type") or "", attr(r, "Target") or "", attr(r, "TargetMode")) for r in root.iter() if local(r) == "Relationship"]


def resolve(base, target):
    if target.startswith("/"):
        return target[1:]
    parts = base.split("/")[:-1]
    for seg in target.split("/"):
        if seg == "..":
            if parts:
                parts.pop()
        elif seg != ".":
            parts.append(seg)
    return "/".join(parts)


def text_parts(z):
    names = set(z.namelist())
    main = next((t for ty, t, _ in relationships(z.read("_rels/.rels")) if ty.endswith("/officeDocument")), None)
    if main is None:
        raise SystemExit("Not a Word document: no main part.")
    main = resolve("", main)
    parts = [main]
    rels = posixpath.join(posixpath.dirname(main), "_rels", posixpath.basename(main) + ".rels")
    if rels in names:
        for ty, target, mode in relationships(z.read(rels)):
            if mode == "External":
                continue
            if ty.rsplit("/", 1)[-1] not in ("header", "footer", "footnotes", "endnotes"):
                continue
            p = resolve(main, target)
            if p in names and p not in parts:
                parts.append(p)
    return parts


def paragraphs(root):
    """Every w:p in document order, not those in mc:Fallback copies (iterative:
    tables can nest thousands deep)."""
    skip = set()
    for fb in root.iter():
        if local(fb) == "Fallback":
            skip.update(id(e) for e in fb.iter())
    return [p for p in root.iter() if local(p) == "p" and id(p) not in skip]


def extract(path, lang):
    with zipfile.ZipFile(path) as z:
        segs = []
        for part in text_parts(z):
            root = ET.fromstring(z.read(part))
            for p in paragraphs(root):
                items = walk(p)
                if translatable(items):
                    segs.append(markup(items))
    out = [HEADER.format(document=posixpath.basename(path.replace("\\", "/")), lang=lang)]
    for n, src in enumerate(segs, 1):
        out.append(f"\n## {n}\n" + "".join(f"> {l}\n" for l in src.split("\n")) + "\n")
    return "".join(out), len(segs)


# ── Checking a translated file ────────────────────────────────────────────

HEADING = re.compile(r"^## (\d+)(?:\s+[·|:-]?\s*(\S+))?\s*$")


def parse(md):
    lines = md.splitlines()
    k = 0
    if lines and lines[0].strip() == "---":
        k = 1
        while k < len(lines) and lines[k].strip() != "---":
            k += 1
        k += 1
    segs, cur, in_comment = [], None, False
    for line in lines[k:]:
        l = line.rstrip()
        if in_comment:
            in_comment = "-->" not in l
            continue
        if l.lstrip().startswith("<!--"):
            in_comment = "-->" not in l
            continue
        m = HEADING.match(l)
        if m:
            cur = {"n": int(m.group(1)), "src": [], "tgt": [], "past": False}
            segs.append(cur)
            continue
        if cur is None:
            continue
        if not cur["past"] and l.startswith(">"):
            q = l[1:]
            cur["src"].append(q[1:] if q.startswith(" ") else q)
            continue
        if cur["src"] or l.strip():
            cur["past"] = True
        if cur["past"] and l.strip():
            cur["tgt"].append(l.strip())
    return segs


PLACEHOLDER = re.compile(r"(?<!\\)\{(\d{1,4}|tab)\}")


def marks(s):
    return sorted(PLACEHOLDER.findall(s))


def open_emphasis(s):
    """Whether bold or italic is left open, reading the stars as Babel does."""
    t = re.sub(r"\\.", "", s)
    b = i = False
    k = 0
    while k < len(t):
        if t[k] != "*":
            k += 1
            continue
        run = len(t[k:]) - len(t[k:].lstrip("*"))
        before = t[k - 1] if k else " "
        after = t[k + run] if k + run < len(t) else " "
        if run == 1 and before.isspace() and after.isspace():
            k += 1
            continue
        left = run
        while left > 0:
            if left >= 3:
                b, i, left = not b, not i, left - 3
            elif left == 2:
                b, left = not b, 0
            else:
                i, left = not i, 0
        k += run
    return b or i


def links(s):
    """How many […] link texts; -1 when the brackets don't pair up."""
    t = re.sub(r"\\.", "", s)
    depth, n = 0, 0
    for c in t:
        if c == "[":
            if depth:
                return -1
            depth, n = 1, n + 1
        elif c == "]":
            if not depth:
                return -1
            depth = 0
    return -1 if depth else n


def check(path):
    segs = parse(open(path, encoding="utf-8").read())
    if not segs:
        print("No segments found.")
        return 1
    problems = []
    for s in segs:
        src, tgt = "\n".join(s["src"]), "\n".join(s["tgt"])
        if not tgt:
            problems.append(f"## {s['n']}: not translated")
            continue
        if marks(src) != marks(tgt):
            problems.append(f"## {s['n']}: placeholders {marks(src)} in the original, {marks(tgt)} in the translation")
        if open_emphasis(tgt):
            problems.append(f"## {s['n']}: unbalanced * or ** in the translation")
        if links(src) != links(tgt):
            problems.append(f"## {s['n']}: {links(src)} link(s) […] in the original, {links(tgt)} in the translation")
        if tgt.count("\n") != src.count("\n"):
            problems.append(f"## {s['n']}: {src.count(chr(10)) + 1} line(s) in the original, {tgt.count(chr(10)) + 1} in the translation (only a problem if unintended)")
    numbers = [s["n"] for s in segs]
    if numbers != list(range(1, len(segs) + 1)):
        problems.append("Segment numbers are not 1, 2, 3… in order.")
    done = sum(1 for s in segs if s["tgt"])
    print(f"{done} of {len(segs)} segments translated.")
    for p in problems:
        print(p)
    return 1 if any("not translated" in p or "placeholders" in p or "unbalanced" in p or "link(s)" in p or "in order" in p for p in problems) else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("extract", help="the document as a segment file, translations empty")
    e.add_argument("docx")
    e.add_argument("--lang", default="", help="the target language, e.g. sv-SE")
    e.add_argument("-o", "--out", help="write here (default: print)")
    c = sub.add_parser("check", help="check a translated segment file")
    c.add_argument("md")
    a = ap.parse_args()
    if a.cmd == "extract":
        md, n = extract(a.docx, a.lang)
        if a.out:
            with open(a.out, "w", encoding="utf-8") as f:
                f.write(md)
            print(f"{n} segments written to {a.out}", file=sys.stderr)
        else:
            sys.stdout.write(md)
        return 0
    return check(a.md)


if __name__ == "__main__":
    sys.exit(main())

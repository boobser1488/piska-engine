#!/usr/bin/env python3
"""Обернуть тексты интерфейса в вызовы перевода (`tr`, `trf!`, `n_`) — механически.

Использует разбор лексем из `extract_ui_strings.py`: те же «кандидаты», что он показывает
как `to-wrap`, но с точным положением в файле. Правит исходники на месте; запускайте на
чистом дереве (в git видно, что изменилось), потом `cargo check`.

    python tools/wrap_ui_strings.py --scope editor --files hierarchy.rs inspector.rs
    python tools/wrap_ui_strings.py --scope hub --dry          # только отчёт

Что делает:
* `format!("… {}/{name}/{x:.1} …", …)` → `trf!("… {}/{name}/{x} …", …)` — тот же `String`;
  форматы `{:.1}` / `{:?}` переносятся в аргумент (`format!("{:.1}", x)`), безымянные
  `{}` остаются безымянными (подстановка по порядку), `{name}` получает `name = name`;
* литерал — аргумент функции интерфейса → `tr("…")`; если функция из этого проекта
  принимает `&str` (сигнатуры читаются из исходников) — `&tr("…")`; `&'static str` и
  литералы вне вызовов (ветки `match`, константы, поля) → `n_("…")`: ключ помечен, а `tr`
  нужно добавить там, где текст показывают (список печатается в конце);
* добавляет `use …::{tr, trf, n_};` после последнего `use` или в начало файла.
"""
import argparse
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import extract_ui_strings as ex  # noqa: E402

ROOT = ex.ROOT

# Методы и функции egui: принимают `impl Into<WidgetText>` / `impl Into<String>` — подходит `tr(..)`.
EGUI = {
    "label", "heading", "button", "small_button", "colored_label", "link", "monospace", "checkbox", "radio",
    "radio_value", "selectable_label", "selectable_value", "menu_button", "collapsing", "on_hover_text",
    "on_disabled_hover_text", "hint_text", "prefix", "suffix", "selected_text", "from_label", "shortcut_text",
    "RichText::new", "Button::new", "Window::new", "Checkbox::new", "SelectableLabel::new", "Label::new",
    "ComboBox::from_label", "new", "text", "title", "heading", "strong", "weak", "small", "monospace",
}
SAFE_METHODS_AFTER = {"to_string", "to_owned", "into", "into_owned", "clone", "as_str", "as_ref", "to_uppercase", "to_lowercase"}
DIRECT_WRAPPERS = {"Some", "Ok", "Err", "unwrap_or", "unwrap_or_else", "map_or", "map_or_else", "Option", "default"}

IMPORT_PATH = {"editor": "crate::i18n", "hub": "engine_editor::kit::i18n"}


# ---------------------------------------------------------------------------------------
# Сигнатуры функций проекта
# ---------------------------------------------------------------------------------------

def param_kind(type_tokens):
    text = "".join(t.value if t.kind != "life" else t.value + " " for t in type_tokens)
    text = text.replace(" ", "")
    if text.startswith("&'static"):
        return "static"
    if text.startswith("&") and ("str" in text and not text.startswith("&mut")):
        return "ref" if re.fullmatch(r"&('[a-z_]+)?str", text) else "other"
    if text == "String":
        return "string"
    if text.startswith("impl") or text.startswith("Cow") or re.fullmatch(r"[A-Z]", text):
        return "generic"
    return "other"


def collect_signatures(paths):
    """{имя: {"method": [список видов параметров по вызовам], "free": [...]}}"""
    sigs = {}
    for path in paths:
        with open(path, encoding="utf-8", newline="") as f:
            src = f.read()
        toks, _ = ex.lex(src)
        n = len(toks)
        i = 0
        while i < n - 2:
            if toks[i].kind == "id" and toks[i].value == "fn" and toks[i + 1].kind == "id":
                name = toks[i + 1].value
                j = i + 2
                # пропустить <generics>
                if toks[j].kind == "p" and toks[j].value == "<":
                    depth = 0
                    while j < n:
                        if toks[j].value == "<":
                            depth += 1
                        elif toks[j].value == ">":
                            depth -= 1
                            if depth == 0:
                                j += 1
                                break
                        j += 1
                if j < n and toks[j].value == "(":
                    depth, k, params, cur = 0, j, [], []
                    angle = 0
                    while k < n:
                        t = toks[k]
                        if t.kind == "p":
                            if t.value in "([{":
                                depth += 1
                                if depth > 1:
                                    cur.append(t)
                            elif t.value in ")]}":
                                depth -= 1
                                if depth == 0:
                                    if cur:
                                        params.append(cur)
                                    break
                                cur.append(t)
                            elif t.value == "<":
                                angle += 1
                                cur.append(t)
                            elif t.value == ">" and not (cur and cur[-1].value == "-"):
                                angle -= 1
                                cur.append(t)
                            elif t.value == "," and depth == 1 and angle <= 0:
                                params.append(cur)
                                cur = []
                            else:
                                cur.append(t)
                        else:
                            cur.append(t)
                        k += 1
                    is_method = False
                    kinds = []
                    for p in params:
                        vals = [t.value for t in p]
                        if "self" in vals[:3] and (":" not in vals or vals.index("self") < vals.index(":") if ":" in vals else True):
                            is_method = True
                            continue
                        if ":" in vals:
                            idx = vals.index(":")
                            kinds.append(param_kind(p[idx + 1:]))
                        else:
                            kinds.append("other")
                    slot = sigs.setdefault(name, {"method": [], "free": []})
                    slot["method" if is_method else "free"].append(kinds)
                    i = k
            i += 1
    return sigs


def kind_at(defs, arg):
    """Вид параметра `arg` по всем определениям; None — неизвестно."""
    found = set()
    for kinds in defs:
        if arg is None:
            found.update(k for k in kinds if k in ("ref", "static", "string"))
        elif arg < len(kinds):
            found.add(kinds[arg])
    if not found:
        return None
    for k in ("static", "ref", "string", "generic"):
        if k in found:
            return k
    return "other"


# ---------------------------------------------------------------------------------------
# Разбор format!
# ---------------------------------------------------------------------------------------

PH = re.compile(r"\{\{|\}\}|\{([^{}]*)\}")


def split_args(toks, open_idx):
    """Индексы токенов аргументов макроса/вызова: [(first, last)], close_idx."""
    depth = 0
    args, start = [], open_idx + 1
    k = open_idx
    while k < len(toks):
        t = toks[k]
        if t.kind == "p":
            if t.value in "([{":
                depth += 1
            elif t.value in ")]}":
                depth -= 1
                if depth == 0:
                    if start <= k - 1:
                        args.append((start, k - 1))
                    return args, k
            elif t.value == "," and depth == 1:
                args.append((start, k - 1))
                start = k + 1
        k += 1
    return args, -1


def rewrite_format(src, h, edits, notes):
    toks, t = h.toks, h.tok
    name, qual, arg, is_macro, cidx, oidx = h.call
    args, close = split_args(toks, oidx)
    if close < 0 or not args:
        notes.append(f"{h.file}:{h.line} format!: cannot parse")
        return False
    raw = src[t.start:t.end]
    positional, named = [], {}
    for a, b in args[1:]:
        if toks[a].kind == "id" and a + 1 <= b and toks[a + 1].value == "=" and not (a + 2 <= b and toks[a + 2].value == "="):
            named[toks[a].value] = (a, b)
        else:
            positional.append((a, b))
    new_raw, pos = [], 0
    inline = []  # (имя, формат)
    pos_i = 0
    spec_wraps = []  # (индекс токена начала, конца, формат)
    for m in PH.finditer(raw):
        new_raw.append(raw[pos:m.start()])
        pos = m.end()
        whole = m.group(0)
        if whole in ("{{", "}}"):
            new_raw.append(whole)
            continue
        content = m.group(1)
        arg_part, _, spec = content.partition(":")
        arg_part = arg_part.strip()
        if arg_part == "":
            if pos_i >= len(positional):
                notes.append(f"{h.file}:{h.line} format!: more {{}} than arguments")
                return False
            if spec:
                a, b = positional[pos_i]
                spec_wraps.append((a, b, spec))
            pos_i += 1
            new_raw.append("{}")
        elif arg_part.isdigit():
            if spec:
                notes.append(f"{h.file}:{h.line} format!: {{N:spec}} needs manual work")
                return False
            new_raw.append("{" + arg_part + "}")
        else:
            if arg_part in named:
                if spec:
                    a, b = named[arg_part]
                    spec_wraps.append((a + 2, b, spec))
            else:
                inline.append((arg_part, spec))
            new_raw.append("{" + arg_part + "}")
    new_raw.append(raw[pos:])
    new_raw = "".join(new_raw)
    if positional and (named or inline):
        # Смешанные `{}` и `{имя}`: безымянные получают имена p0, p1… (то же делает norm_key
        # для каталога), все аргументы становятся именованными.
        counter = 0
        def rename(m):
            nonlocal counter
            if m.group(0) in ("{{", "}}"):
                return m.group(0)
            if m.group(1).partition(":")[0].strip() == "":
                counter += 1
                return "{p%d}" % (counter - 1)
            return m.group(0)
        new_raw = PH.sub(rename, new_raw)
        for i, (a, b) in enumerate(positional):
            edits.append((toks[a].start, toks[a].start, "p%d = " % i))
        positional = []
        pos_i = 0
    # правки: имя макроса, шаблон, аргументы
    edits.append((toks[cidx].start, toks[cidx].end, "trf"))
    if new_raw != raw:
        edits.append((t.start, t.end, new_raw))
    for a, b, spec in spec_wraps:
        edits.append((toks[a].start, toks[a].start, f'format!("{{:{spec}}}", '))
        edits.append((toks[b].end, toks[b].end, ")"))
    if inline:
        seen, extra = set(), []
        for nm, spec in inline:
            if nm in seen:
                continue
            seen.add(nm)
            value = f'format!("{{:{spec}}}", {nm})' if spec else nm
            extra.append(f"{nm} = {value}")
        close_tok = toks[close]
        before = toks[close - 1]
        text = ", ".join(extra)
        edits.append((close_tok.start, close_tok.start, text if before.value == "," else ", " + text))
    return True


# ---------------------------------------------------------------------------------------
# Основной проход
# ---------------------------------------------------------------------------------------

def in_const_item(toks, idx):
    """Литерал стоит внутри `const` / `static` (там нельзя вызывать `tr_s`)."""
    depth = 0
    j = idx - 1
    while j >= 0:
        t = toks[j]
        if t.kind == "p":
            if t.value in ")]}":
                depth += 1
            elif t.value in "([{":
                if depth == 0:
                    if t.value == "{":
                        prev = toks[j - 1] if j > 0 else None
                        # `Тип { поле: "…" }` — литерал структуры, ищем `const` выше;
                        # иначе это тело функции / блока
                        if not (prev is not None and prev.kind == "id" and prev.value[:1].isupper()):
                            return False
                    # открывающая скобка выше литерала: продолжаем наружу
                else:
                    depth -= 1
            elif t.value == ";" and depth == 0:
                return False
        elif t.kind == "id" and depth == 0 and t.value in ("const", "static"):
            # `const fn` — не данные
            nxt = toks[j + 1].value if j + 1 < len(toks) else ""
            return nxt != "fn"
        j -= 1
    return False


def plan_file(path, rel, scope, sigs, ignore, report):
    with open(path, encoding="utf-8", newline="") as f:
        src = f.read()
    hits = [h for h in ex.scan_source(path, rel, ignore) if h.kind == "candidate" and h.tok is not None]
    edits, notes, used = [], report["notes"], set()
    for h in hits:
        toks, t = h.toks, h.tok
        name, qual, arg, is_macro, cidx, oidx = h.call
        idx = toks.index(t)
        if is_macro and name == "format" and h.via == "format!":
            if rewrite_format(src, h, edits, notes):
                used.add("trf")
                report["format"] += 1
            else:
                report["manual"].append(f"{rel}:{h.line} {h.key[:60]!r}")
            continue
        nxt = toks[idx + 1] if idx + 1 < len(toks) else None
        after_dot = nxt is not None and nxt.value == "."
        if after_dot:
            meth = toks[idx + 2].value if idx + 2 < len(toks) else ""
            if meth not in SAFE_METHODS_AFTER:
                report["skipped"].append(f"{rel}:{h.line} {h.key[:50]!r} (method .{meth})")
                continue
        lit = src[t.start:t.end]
        # `tr_s("…")` — `&'static str`: подходит везде, где был литерал (аргумент `&str`, ветки
        # `if`/`match`, поля, возврат). Только в `const` / `static` вызов функции невозможен —
        # там ключ лишь помечается `n_`, а `tr_s` ставится там, где текст показывают.
        if in_const_item(toks, idx):
            new, used_name, mode = f"n_({lit})", "n_", "mark"
            report["marked"].append(f"{rel}:{h.line} {h.key[:60]!r} ({name})")
        else:
            new, used_name, mode = f"tr_s({lit})", "tr_s", "tr"
        used.add(used_name)
        edits.append((t.start, t.end, new))
        report[mode] += 1
    return src, edits, used


def apply_edits(src, edits):
    # одинаковые позиции вставки идут в порядке добавления; применяем с конца
    ordered = sorted(enumerate(edits), key=lambda e: (e[1][0], e[1][1], e[0]), reverse=True)
    out = src
    last_start = None
    for _, (s, e, text) in ordered:
        out = out[:s] + text + out[e:]
    return out


def add_import(src, names, path):
    if not names:
        return src
    wanted = ", ".join(sorted(names, key=lambda n: ("tr", "tr_s", "trf", "n_").index(n) if n in ("tr", "tr_s", "trf", "n_") else 9))
    line = f"use {path}::{{{wanted}}};"
    if re.search(r"^use %s::" % re.escape(path), src, re.M):
        # уже есть импорт слоя: дописать недостающие имена
        m = re.search(r"^use %s::\{([^}]*)\};" % re.escape(path), src, re.M)
        if m:
            have = {x.strip() for x in m.group(1).split(",") if x.strip()}
            all_names = sorted(have | set(names))
            return src[:m.start()] + f"use {path}::{{{', '.join(all_names)}}};" + src[m.end():]
        m = re.search(r"^use %s::(\w+);" % re.escape(path), src, re.M)
        if m:
            all_names = sorted({m.group(1)} | set(names))
            return src[:m.start()] + f"use {path}::{{{', '.join(all_names)}}};" + src[m.end():]
        return src
    nl = "\r\n" if "\r\n" in src else "\n"
    m = re.search(r"^use ", src, re.M)
    if m:
        return src[:m.start()] + line + nl + src[m.start():]
    # нет `use`: после шапки из `//!` и `#![…]`
    lines = src.split(nl)
    i = 0
    while i < len(lines) and (lines[i].startswith("//!") or lines[i].startswith("#![") or not lines[i].strip()):
        i += 1
    lines.insert(i, line)
    lines.insert(i + 1, "")
    return nl.join(lines)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scope", required=True, choices=["editor", "hub"])
    ap.add_argument("--files", nargs="*", help="имена файлов (hierarchy.rs, settings/input.rs); по умолчанию все")
    ap.add_argument("--dry", action="store_true")
    args = ap.parse_args()
    ignore = ex.load_ignore()
    all_paths = list(ex.walk(ex.SCOPES[args.scope]))
    sigs = collect_signatures(all_paths)
    wanted = None
    if args.files:
        wanted = {w.replace("\\", "/") for w in args.files}
    report = {"format": 0, "tr": 0, "ref": 0, "string": 0, "mark": 0, "notes": [], "manual": [], "marked": [], "skipped": []}
    changed = 0
    for path in all_paths:
        rel = os.path.relpath(path, ROOT).replace("\\", "/")
        if wanted and not any(rel.endswith("/" + w) or rel == w for w in wanted):
            continue
        src, edits, used = plan_file(path, rel, args.scope, sigs, ignore, report)
        if not edits:
            continue
        out = apply_edits(src, edits)
        out = add_import(out, used, IMPORT_PATH[args.scope])
        changed += 1
        if not args.dry:
            with open(path, "w", encoding="utf-8", newline="") as f:
                f.write(out)
    for key in ("notes", "manual", "skipped", "marked"):
        for line in report[key]:
            print(f"{key}: {line}")
    print(f"files changed: {changed}; format! -> trf!: {report['format']}; tr(): {report['tr']}; &tr(): {report['ref']}; "
          f"tr().into_owned(): {report['string']}; n_() marks: {report['mark']}")


if __name__ == "__main__":
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    main()

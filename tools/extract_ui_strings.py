#!/usr/bin/env python3
"""Сбор текстов интерфейса для перевода и проверка каталога `ru.po`.

Сканирует исходники интерфейса (редактор, хаб, редактор моделей) настоящим разбором
лексем Rust (комментарии, строки, сырые строки, символы — не регулярками по тексту) и
находит два рода строк:

* **обёрнутые** — аргументы `tr("…")`, `tr_ctx("ctx", "…")`, `trf!("…")`, `trn!("one", "other", n)`,
  `tr_n`, `tr_fmt`, `n_("…")` и ключи из комментариев `// i18n-keys: "…" "…"`;
* **кандидаты** — ещё не обёрнутые строки, которые похожи на видимый текст: аргументы
  известных функций интерфейса (`ui.label`, `button`, `on_hover_text`, `RichText::new`,
  строки настроек `s.toggle`, …) и «человекоподобные» литералы в остальном коде.

Не считаются: комментарии, `#[cfg(test)]` и `#[test]`, атрибуты, макросы журнала
(`info!`, `warn!` …), идентификаторы (`Id::new`, `id_salt`), пути, шаблоны `match`.
Ложные срабатывания добавляются в `tools/ui_strings_ignore.txt` (по строке на литерал).

    python tools/extract_ui_strings.py                 # список ключей: файл:строка
    python tools/extract_ui_strings.py --stats         # число мест по файлам
    python tools/extract_ui_strings.py --check         # сверка с каталогом, код выхода 1
    python tools/extract_ui_strings.py --check --scope modeler
    python tools/extract_ui_strings.py --update        # дописать недостающие ключи в ru.po (пустой msgstr)

`--check` сообщает: *missing* — ключ есть в коде, нет в каталоге (или перевод пуст);
*unused* — ключ есть в каталоге, нет в коде (только при `--scope all`);
*placeholders* — в переводе другие `{имена}`, чем в ключе.
"""
import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

SCOPES = {
    "editor": ["crates/engine-editor/src"],
    "hub": ["tools/piska-hub/src"],
    "installer": ["tools/piska-installer/src"],
    "modeler": ["modeler/crates/engine-modeler/src/ui", "modeler/crates/piska-modeler/src"],
}
CATALOGUE = "crates/engine-i18n/locale/ru.po"
IGNORE_FILE = "tools/ui_strings_ignore.txt"

# --- обёртки: имя -> номера аргументов-ключей ---------------------------------------
WRAPPERS = {
    "tr": (0,),
    "tr_s": (0,),
    "tr_ctx_s": (0, 1),
    "tr_in": (1,),
    "tr_fmt": (0,),
    "trf": (0,),
    "tr_n": (0, 1),
    "tr_n_in": (1, 2),
    "trn": (0, 1),
    "n_": (0,),
}
CTX_WRAPPERS = {"tr_ctx": (0, 1), "tr_ctx_s": (0, 1), "tr_ctx_in": (1, 2)}  # (номер контекста, номер ключа)
# Помощники окна моделей, переводящие аргумент внутри: имя -> номера аргументов-текстов.
DATA_FILES = {"crates/engine-editor/src/catalog.rs", "crates/engine-editor/src/add_menu.rs"}
AUTO_TR_PREFIX = "modeler/crates/engine-modeler/src/ui/"
AUTO_TR = {"tip": (0, 2), "icon_button": (5,), "section": (1,), "row": (1,), "vec3_fields": (1,),
           "item": (1,), "edit": (0,), "begin": (0,)}

# --- функции интерфейса: имя -> номера аргументов-текстов (None — любой) -------------
UI_CALLS = {
    "label": (0,), "heading": (0,), "button": (0, 1), "small_button": (0,),
    "colored_label": (1,), "hyperlink_to": (0,), "link": (0,), "monospace": (0,),
    "checkbox": (1,), "radio": (1,), "radio_value": (2,), "selectable_label": (1,),
    "selectable_value": (2,), "menu_button": (0,), "collapsing": (0,), "on_hover_text": (0,),
    "on_disabled_hover_text": (0,), "hint_text": (0,), "prefix": (0,), "suffix": (0,),
    "selected_text": (0,), "from_label": (0,), "title": (0,), "text": (0,),
    "shortcut_text": (0,), "add_enabled_button": (1,), "separator_text": (0,),
    "RichText::new": (0,), "Button::new": (0,), "Window::new": (0,), "Checkbox::new": (1,),
    "SelectableLabel::new": (1,), "Label::new": (0,), "WidgetText::from": (0,),
    "ComboBox::from_label": (0,), "Hyperlink::from_label_and_url": (0,),
    # общие виджеты редактора и хаба
    "group": (0,), "toggle": (0, 1), "number": (0, 1), "percent": (0, 1), "combo": (0, 1),
    "row": (0, 1), "text_field": (0, 1), "path_field": (0, 1), "slider": (0, 1),
    "dialog_title": (3,), "section": (1,), "section_header": (0,), "field_label": (0,),
    "icon_button": (5,), "tip": (0, 2), "swatch": (), "chip": (0,), "pill": (0,),
    "message": (0,), "error": (0,), "toast": (0,), "status": (0,), "set_status": (0,),
    "empty_state": (0, 1), "primary_button": (0,), "secondary_button": (0,),
    "ghost_button": (0,), "danger_button": (0,), "tab": (0,), "search_field": (0,),
}
UI_CALLS = {k: (v if v else None) for k, v in UI_CALLS.items()}

# --- вызовы, строки в которых никогда не текст интерфейса --------------------------
SKIP_CALLS = {
    "info", "warn", "error_log", "debug", "trace", "log", "println", "eprintln", "print",
    "eprint", "panic", "assert", "assert_eq", "assert_ne", "debug_assert", "debug_assert_eq",
    "write", "writeln", "unreachable", "todo", "unimplemented", "stringify", "concat",
    "include_str", "include_bytes", "env", "option_env", "cfg", "compile_error", "matches",
    "Id::new", "new_id", "id_salt", "from_id_salt", "push_id", "with_id", "id", "Id::from",
    "join", "extension", "set_extension", "with_extension", "ends_with", "starts_with",
    "contains", "strip_prefix", "strip_suffix", "split", "split_once", "rsplit_once", "replace",
    "replacen", "trim_matches", "trim_start_matches", "trim_end_matches", "find", "rfind",
    "eq_ignore_ascii_case", "var", "var_os", "env::var", "env::var_os", "Command::new", "arg",
    "args", "current_dir", "Path::new", "PathBuf::from", "File::open", "File::create",
    "read_to_string", "write_to", "get_temp", "insert_temp", "remove_temp", "get_persisted",
    "get_persisted_mut_or_default", "data", "expect", "unwrap_or_default", "map_err_msg",
    "from_str", "parse", "parse_entry", "set", "get", "key", "set_key", "insert", "entry",
    "remove", "and_then", "ok_or", "line", "register", "register_type", "add_class", "class",
    "file_name", "file_stem", "ron", "ext", "has_extension", "type_name", "display_name_of",
    "find_component", "get_component", "has_component", "tag", "layer", "load", "save_as_key",
    "with_label", "mono_id", "salt", "scope", "from_id", "id_source", "id_source_str", "CStr::from_bytes",
    "static_id", "cache_key", "snapshot_key", "kind", "component_named", "type_registry",
    "format_ident", "from_utf8", "from_utf8_lossy",
}

LOG_MACROS = {"info", "warn", "error", "debug", "trace", "log"}

IDENT_LIKE = re.compile(r"^[a-z0-9_\-./:#\\*%{}<>=\[\]|@+,;!?()&$^~`\s]*$")
CAMEL = re.compile(r"^[A-Z][a-z0-9]+([A-Z][a-z0-9]*)+$")
HAS_LETTER = re.compile(r"[A-Za-zА-Яа-яЁё]")


# =======================================================================================
# Лексер Rust (достаточный для поиска строк)
# =======================================================================================

class Tok:
    __slots__ = ("kind", "value", "line", "start", "end")

    def __init__(self, kind, value, line):
        self.kind, self.value, self.line = kind, value, line
        self.start = self.end = 0

    def __repr__(self):
        return f"{self.kind}:{self.value!r}@{self.line}"


ESC = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}


def lex(src):
    """Возвращает (токены, комментарии[(строка, текст)])."""
    toks, comments = [], []
    i, n, line = 0, len(src), 1
    while i < n:
        c = src[i]
        begin, before = i, len(toks)
        if c == "\n":
            line += 1
            i += 1
        elif c in " \t\r":
            i += 1
        elif src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            comments.append((line, src[i:j]))
            i = j
        elif src.startswith("/*", i):
            depth, j, start_line = 1, i + 2, line
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    if src[j] == "\n":
                        line += 1
                    j += 1
            comments.append((start_line, src[i:j]))
            i = j
        elif c == '"' or (c == "b" and src.startswith('b"', i)):
            is_bytes = c == "b"
            j = i + (2 if is_bytes else 1)
            start_line, out = line, []
            while j < n and src[j] != '"':
                if src[j] == "\\":
                    d = src[j + 1] if j + 1 < n else ""
                    if d == "\n" or (d == "\r" and src[j + 2:j + 3] == "\n"):  # перенос строки в литерале (LF и CRLF)
                        line += 1
                        j += 2
                        while j < n and src[j] in " \t\r\n":
                            if src[j] == "\n":
                                line += 1
                            j += 1
                        continue
                    if d == "u" and src[j + 2:j + 3] == "{":
                        end = src.index("}", j)
                        try:
                            out.append(chr(int(src[j + 3:end], 16)))
                        except ValueError:
                            pass
                        j = end + 1
                        continue
                    if d == "x":
                        try:
                            out.append(chr(int(src[j + 2:j + 4], 16)))
                        except ValueError:
                            pass
                        j += 4
                        continue
                    out.append(ESC.get(d, d))
                    j += 2
                    continue
                if src[j] == "\n":
                    line += 1
                out.append(src[j])
                j += 1
            toks.append(Tok("bytes" if is_bytes else "str", "".join(out).replace("\r", ""), start_line))
            i = j + 1
        elif c == "r" and re.match(r'r#*"', src[i:i + 8]):
            m = re.match(r'r(#*)"', src[i:])
            hashes = m.group(1)
            end_marker = '"' + hashes
            j = src.find(end_marker, i + len(m.group(0)))
            j = n if j < 0 else j
            body = src[i + len(m.group(0)):j]
            toks.append(Tok("str", body.replace("\r", ""), line))
            line += body.count("\n")
            i = j + len(end_marker)
        elif c == "'":
            # символ или время жизни
            if src[i + 1:i + 2] == "\\":
                j = src.index("'", i + 2)
                toks.append(Tok("char", src[i:j + 1], line))
                i = j + 1
            elif src[i + 2:i + 3] == "'":
                toks.append(Tok("char", src[i:i + 3], line))
                i += 3
            else:
                m = re.match(r"'[A-Za-z_][A-Za-z0-9_]*", src[i:])
                if m:
                    toks.append(Tok("life", m.group(0), line))
                    i += len(m.group(0))
                else:
                    i += 1
        elif c.isalpha() or c == "_":
            m = re.match(r"[A-Za-z_][A-Za-z0-9_]*", src[i:])
            word = m.group(0)
            toks.append(Tok("id", word, line))
            i += len(word)
        elif c.isdigit():
            m = re.match(r"[0-9][0-9A-Za-z_.]*", src[i:])
            word = m.group(0)
            # 1..2 : не съедать диапазон
            if ".." in word:
                word = word[:word.index("..")]
            toks.append(Tok("num", word, line))
            i += len(word)
        else:
            toks.append(Tok("p", c, line))
            i += 1
        if len(toks) > before:
            toks[-1].start, toks[-1].end = begin, i
    return toks, comments


# =======================================================================================
# Разбор токенов
# =======================================================================================

OPEN = {"(": ")", "[": "]", "{": "}"}
CLOSE = {")", "]", "}"}


def skip_item(toks, i):
    """С токена `i` пропустить один элемент: до `;` или парной `}` первого блока."""
    n = len(toks)
    depth = 0
    while i < n:
        t = toks[i]
        if t.kind == "p":
            if t.value in "([":
                depth += 1
            elif t.value in ")]":
                depth -= 1
            elif t.value == "{":
                d = 1
                i += 1
                while i < n and d:
                    if toks[i].kind == "p":
                        if toks[i].value == "{":
                            d += 1
                        elif toks[i].value == "}":
                            d -= 1
                    i += 1
                return i
            elif t.value == ";" and depth <= 0:
                return i + 1
        i += 1
    return n


def strip_tests_and_attrs(toks):
    """Убрать `#[cfg(test)]`/`#[test]` вместе с элементом, остальные атрибуты — отдельно."""
    out = []
    i, n = 0, len(toks)
    while i < n:
        t = toks[i]
        if t.kind == "p" and t.value == "#":
            j = i + 1
            if j < n and toks[j].kind == "p" and toks[j].value == "!":
                j += 1
            if j < n and toks[j].kind == "p" and toks[j].value == "[":
                d, k = 0, j
                while k < n:
                    if toks[k].kind == "p" and toks[k].value == "[":
                        d += 1
                    elif toks[k].kind == "p" and toks[k].value == "]":
                        d -= 1
                        if d == 0:
                            break
                    k += 1
                attr = [x.value for x in toks[j:k + 1]]
                text = "".join(attr)
                is_test = text in ("[test]", "[cfg(test)]") or text.startswith("[cfg(test") or "tokio::test" in text
                if is_test and toks[i + 1].value != "!":
                    i = skip_item(toks, k + 1)
                    continue
                i = k + 1  # прочие атрибуты (в т. ч. `#[error("…")]`) не трогаем
                continue
        out.append(t)
        i += 1
    return out


def callee_of(toks, i):
    """Для литерала `toks[i]`: (вызывающая функция, номер аргумента | None, это макрос)."""
    depth = 0
    j = i - 1
    commas = 0
    transparent = False  # были внутри `{}` / `[]` — номер аргумента неизвестен
    while j >= 0:
        t = toks[j]
        if t.kind == "p":
            v = t.value
            if v in CLOSE:
                depth += 1
            elif v in "([{":
                if depth == 0:
                    if v == "(":
                        # вызов?
                        k = j - 1
                        is_macro = False
                        if k >= 0 and toks[k].kind == "p" and toks[k].value == "!":
                            is_macro = True
                            k -= 1
                        if k >= 0 and toks[k].kind == "id" and toks[k].value not in ("if", "match", "while", "return", "in", "for", "let", "else"):
                            name = toks[k].value
                            qual = name
                            if k >= 2 and toks[k - 1].value == ":" and toks[k - 2].value == ":" and k >= 3 and toks[k - 3].kind == "id":
                                qual = toks[k - 3].value + "::" + name
                            return name, qual, (None if transparent else commas), is_macro, k, j
                        # кортеж / скобки: смотрим выше, номер аргумента неизвестен
                        transparent = True
                        commas = 0
                        j -= 1
                        continue
                    transparent = True
                    commas = 0
                    j -= 1
                    continue
                depth -= 1
            elif v == "," and depth == 0 and not transparent:
                commas += 1
            elif v == ";" and depth == 0:
                return None, None, None, False, -1, -1
        j -= 1
    return None, None, None, False, -1, -1


def human_like(s):
    if not HAS_LETTER.search(s):
        return False
    st = s.strip()
    if not st:
        return False
    if IDENT_LIKE.match(st) and not any(ch.isupper() for ch in st):
        return False
    if "::" in st or st.startswith(("http", "www.", "\\\\", "./", "../")):
        return False
    if CAMEL.match(st):
        return False
    if "/" in st and " " not in st:
        return False
    if re.match(r"^[A-Za-z0-9_]+\.[a-z0-9]{1,5}$", st):
        return False
    if re.match(r"^[A-Z0-9_]+$", st) and len(st) > 3:  # ВЕРХНИЙ_РЕГИСТР: константы и ключи
        return False
    if re.match(r"^[#%{}0-9a-fA-F:._\- ]+$", st) and not re.search(r"[g-zG-Z]", st):
        return False
    first = st[0]
    return first.isupper() or (" " in st and re.search(r"[A-Za-z]{3,}", st) is not None)


class Hit:
    __slots__ = ("key", "ctx", "file", "line", "kind", "via", "tok", "toks", "call", "plural")

    def __init__(self, key, ctx, file, line, kind, via):
        self.key, self.ctx, self.file, self.line, self.kind, self.via = key, ctx, file, line, kind, via
        self.tok = self.toks = self.call = self.plural = None


def is_data_template(key):
    """Шаблон `format!` для файлов, id, протокола и форматов — не текст интерфейса."""
    if key.endswith("\n") or key.startswith((";", "    ", "pub ", "open ")):
        return True
    return " " not in key.strip()  # одно слово с подстановкой: имя файла, id, ключ


def template_like(s):
    """Шаблон `format!` без заглавных: `{} elements`, `{count} items selected`."""
    rest = re.sub(r"\{[^{}]*\}", " ", s.replace("{{", " ").replace("}}", " "))
    if "/" in rest and " " not in rest.strip():
        return False
    if "::" in rest or rest.strip().startswith(("http", "-", "--")):
        return False
    return re.search(r"[A-Za-z]{3,}", rest) is not None and re.search(r"[A-Za-z]{3,}.*\s|\s.*[A-Za-z]{3,}|^\s*[A-Za-z]{4,}", rest) is not None


def is_pattern(toks, i):
    """Литерал стоит в образце `match` (`Some("Code") =>`, `"a" | "b"`, `"x" if …`)."""
    j = i + 1
    while j < len(toks) and toks[j].kind == "p" and toks[j].value in ")]":
        j += 1
    if j >= len(toks):
        return False
    t = toks[j]
    if t.kind == "p" and t.value == "|":
        return True
    if t.kind == "p" and t.value == "=" and j + 1 < len(toks) and toks[j + 1].value == ">":
        return True
    # `if` после образца — только если до `=>` нет `;` и `{`
    if t.kind == "id" and t.value == "if":
        k = j
        while k < len(toks) and k < j + 40:
            if toks[k].value == "=" and k + 1 < len(toks) and toks[k + 1].value == ">":
                return True
            if toks[k].value in (";", "{"):
                return False
            k += 1
    return False


def candidate(hit, tok, toks, call):
    """Запомнить, где в исходнике стоит литерал: нужно `wrap_ui_strings.py`."""
    hit.tok, hit.toks, hit.call = tok, toks, call
    return hit


def scan_source(path, rel, ignore):
    with open(path, encoding="utf-8", newline="") as f:
        src = f.read()
    toks, comments = lex(src)
    hits = []
    skip_lines = {l for line, text in comments if "i18n-skip" in text for l in (line, line + 1)}  # `// i18n-skip` — не текст интерфейса
    for line, text in comments:
        mc = re.search(r"i18n-ctx:\s*(\w+)(.*)", text)
        if mc:  # // i18n-ctx: mode "Edit" "Paint"  — ключи с контекстом в таблицах
            for s in re.findall(r'"((?:[^"\\]|\\.)*)"', mc.group(2)):
                hits.append(Hit(s, mc.group(1), rel, line, "wrapped", "marker"))
        m = re.search(r"i18n-keys?:(.*)", text)
        if m:
            for s in re.findall(r'"((?:[^"\\]|\\.)*)"', m.group(1)):
                hits.append(Hit(s.encode().decode("unicode_escape") if "\\" in s else s, None, rel, line, "wrapped", "marker"))
    toks = strip_tests_and_attrs(toks)
    skip_until_semicolon = False
    for i, t in enumerate(toks):
        # `use` / `mod` / `extern` — строк нет
        if t.kind != "str":
            continue
        s = t.value
        if t.line in skip_lines:
            continue
        nxt = toks[i + 1] if i + 1 < len(toks) else None
        if nxt is not None and nxt.kind == "p" and nxt.value in ("|",):
            continue
        if nxt is not None and nxt.kind == "p" and nxt.value == "=" and i + 2 < len(toks) and toks[i + 2].value == ">":
            continue
        if is_pattern(toks, i):
            continue
        # Сравнения (`x == "Material"`) и шаблоны — данные, не текст интерфейса.
        prev2 = [toks[i - 2].value, toks[i - 1].value] if i >= 2 else []
        if prev2 in (["=", "="], ["!", "="]) or (nxt is not None and nxt.value in ("=", "!") and i + 2 < len(toks) and toks[i + 2].value == "="):
            continue
        name, qual, arg, is_macro, callee_idx, open_idx = callee_of(toks, i)
        # --- обёртки
        if name in CTX_WRAPPERS and name is not None:
            ci, ki = CTX_WRAPPERS[name]
            if arg == ki:
                # контекст — предыдущий аргумент-строка
                ctx = None
                for back in range(i - 1, max(i - 4, 0), -1):
                    if toks[back].kind == "str":
                        ctx = toks[back].value
                        break
                hits.append(Hit(s, ctx, rel, t.line, "wrapped", name))
            continue
        if name in WRAPPERS:
            # Во множественном числе ключом служит единственное (`msgid`), второе — `msgid_plural`.
            plural_arg = {"tr_n": 1, "trn": 1, "tr_n_in": 2}.get(name)
            if arg != plural_arg and (arg is None or arg in WRAPPERS[name]):
                h = Hit(s, None, rel, t.line, "wrapped", name)
                if plural_arg is not None and i + 2 < len(toks) and toks[i + 2].kind == "str":
                    h.plural = toks[i + 2].value  # `trn!("{n} item", "{n} items", …)`
                hits.append(h)
            continue
        # Помощники окна моделей, которые переводят свои аргументы сами (`style::tip`,
        # `icon_button`, `section`, `row`, меню `item`, подписи истории правок).
        if rel.startswith(AUTO_TR_PREFIX) and name in AUTO_TR and (arg is None or arg in AUTO_TR[name]) and HAS_LETTER.search(s):
            hits.append(Hit(s, None, rel, t.line, "wrapped", name))
            continue
        if s in ignore or not HAS_LETTER.search(s):
            continue
        # шаблоны файлов (RON, Scenario, код) и испорченная кодировка — не текст интерфейса
        if (s.endswith("\n") and "=" in s) or s.rstrip().endswith(";") or re.search(r"\.(cpp|rs|toml|md)$", s) or s.startswith("# ") or "�" in s or (len(s) > 90 and s.count("\n") > 3) or s.lstrip().startswith(("(\n", "//", "--", "{\n", "[\n")):
            continue
        # --- пропускаемые вызовы
        if name in SKIP_CALLS or qual in SKIP_CALLS:
            continue
        if is_macro and name in LOG_MACROS:
            continue
        if is_macro and name == "format":
            # шаблон: текст интерфейса, только если выглядит как фраза
            if open_idx >= 0 and toks[open_idx + 1] is t:  # шаблон — первый аргумент
                if (human_like(s) or template_like(s)) and not is_data_template(s):
                    hits.append(candidate(Hit(s, None, rel, t.line, "candidate", "format!"), t, toks, (name, qual, arg, is_macro, callee_idx, open_idx)))
            elif human_like(s):  # значение внутри аргументов format!: `if a { "Yes" } else { "No" }`
                hits.append(candidate(Hit(s, None, rel, t.line, "candidate", "format-arg"), t, toks, (name, qual, arg, is_macro, callee_idx, open_idx)))
            continue
        # --- функции интерфейса
        matched = False
        for key in (qual, name):
            if key in UI_CALLS:
                wanted = UI_CALLS[key]
                if wanted is None or arg is None or arg in wanted:
                    hits.append(candidate(Hit(s, None, rel, t.line, "candidate", key), t, toks, (name, qual, arg, is_macro, callee_idx, open_idx)))
                    matched = True
                break
        if not matched and human_like(s):
            # другой аргумент той же функции (номера в таблице — для типичных вызовов) или
            # функция не из таблицы: решает вид самой строки
            hits.append(candidate(Hit(s, None, rel, t.line, "candidate", name or "literal"), t, toks, (name, qual, arg, is_macro, callee_idx, open_idx)))
    if rel in DATA_FILES:
        # Файл-таблица: идентификаторы и имена типов не текст; переводимое помечено `n_` вручную.
        hits = [h for h in hits if h.kind == "wrapped"]
    return hits


def load_ignore():
    path = os.path.join(ROOT, IGNORE_FILE)
    out = set()
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            for line in f:
                line = line.rstrip("\n")
                if line and not line.startswith("#"):
                    out.add(line.replace("\\n", "\n"))
    return out


def walk(scope_dirs):
    for d in scope_dirs:
        base = os.path.join(ROOT, d)
        if os.path.isfile(base):
            yield base
            continue
        for dirpath, dirnames, filenames in os.walk(base):
            dirnames[:] = [x for x in dirnames if x not in ("target", "tests", "examples", "benches")]
            for fn in sorted(filenames):
                if fn.endswith(".rs") and fn not in ("tests.rs", "kit.rs") and not fn.startswith("test_"):
                    yield os.path.join(dirpath, fn)


def collect(scopes):
    ignore = load_ignore()
    hits = []
    for scope in scopes:
        for path in walk(SCOPES[scope]):
            rel = os.path.relpath(path, ROOT).replace("\\", "/")
            for h in scan_source(path, rel, ignore):
                h.via = h.via
                hits.append((scope, h))
    return hits


# =======================================================================================
# Каталог .po
# =======================================================================================

def po_unquote(s):
    s = s.strip()
    if not (s.startswith('"') and s.endswith('"') and len(s) >= 2):
        raise ValueError("expected a quoted string: " + s)
    out, i, body = [], 0, s[1:-1]
    while i < len(body):
        c = body[i]
        if c == "\\":
            i += 1
            d = body[i]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\"}.get(d, d))
        else:
            out.append(c)
        i += 1
    return "".join(out)


def po_quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t") + '"'


def load_catalogue(path):
    """{(ctx, id): [формы]} и список ошибок."""
    entries, errors = {}, []
    if not os.path.exists(path):
        return entries, [f"{path}: no such file"]
    cur = {}
    field = None
    start = 0

    def finish():
        nonlocal cur
        if "id" in cur:
            key = (cur.get("ctx"), cur["id"])
            if key[1] != "":
                if key in entries:
                    errors.append(f"line {start}: duplicate entry {key[1]!r}")
                else:
                    entries[key] = cur.get("str", [""])
        cur = {}

    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    for no, raw in enumerate(lines, 1):
        line = raw.strip()
        if not line:
            finish()
            field = None
            continue
        if line.startswith("#"):
            continue
        m = re.match(r"(msgctxt|msgid_plural|msgid|msgstr(?:\[(\d+)\])?)\s*(.*)$", line)
        try:
            if m:
                kw, idx, rest = m.group(1), m.group(2), m.group(3)
                if kw in ("msgctxt", "msgid") and "str" in cur:
                    finish()
                if kw == "msgid":
                    start = no
                value = po_unquote(rest)
                if kw == "msgctxt":
                    cur["ctx"], field = value, ("ctx",)
                elif kw == "msgid":
                    cur["id"], field = value, ("id",)
                elif kw == "msgid_plural":
                    cur["plural"], field = value, ("plural",)
                else:
                    i = int(idx) if idx else 0
                    forms = cur.setdefault("str", [])
                    while len(forms) <= i:
                        forms.append("")
                    forms[i] = value
                    field = ("str", i)
            elif line.startswith('"') and field:
                value = po_unquote(line)
                if field[0] == "str":
                    cur["str"][field[1]] += value
                else:
                    cur[field[0]] += value
            else:
                errors.append(f"line {no}: cannot parse `{line[:40]}`")
        except (ValueError, IndexError) as e:
            errors.append(f"line {no}: {e}")
    finish()
    return entries, errors


_PH = re.compile(r"\{\{|\}\}|\{([^{}]*)\}")


def strip_specs(text):
    """`{x:.1}` → `{x}`, `{:?}` → `{}`: формат переносится в аргумент (см. wrap_ui_strings.py)."""
    def one(m):
        if m.group(0) in ("{{", "}}"):
            return m.group(0)
        return "{" + m.group(1).partition(":")[0].strip() + "}"
    return _PH.sub(one, text)


def is_mixed(text):
    names = [m.group(1) for m in _PH.finditer(text) if m.group(0) not in ("{{", "}}")]
    return "" in names and any(n for n in names)


def name_positional(text):
    """Смешанный шаблон: безымянные `{}` → `{p0}`, `{p1}`…"""
    count = 0
    def one(m):
        nonlocal count
        if m.group(0) in ("{{", "}}") or m.group(1) != "":
            return m.group(0)
        count += 1
        return "{p%d}" % (count - 1)
    return _PH.sub(one, text)


def norm_key(text):
    """Вид ключа после `wrap_ui_strings.py`."""
    text = strip_specs(text)
    return name_positional(text) if is_mixed(text) else text


def migrate_catalogue(path):
    """Привести ключи и переводы каталога к виду `norm_key` (без форматов, смешанные — с именами)."""
    with open(path, encoding="utf-8", newline="") as f:
        text = f.read().replace("\r\n", "\n")
    blocks = text.split("\n\n")
    out, changed = [], 0
    for b in blocks:
        lines = b.split("\n")
        mixed = False
        for ln in lines:
            if ln.startswith("msgid "):
                try:
                    key = po_unquote(ln[6:])
                except ValueError:
                    continue
                mixed = is_mixed(strip_specs(key))
        new_lines = []
        for ln in lines:
            m = re.match(r"^(msgid_plural |msgid |msgstr(?:\[\d+\])? )(\".*\")$", ln)
            if m:
                value = po_unquote(m.group(2))
                fixed = strip_specs(value)
                if mixed:
                    fixed = name_positional(fixed)
                if fixed != value:
                    changed += 1
                    ln = m.group(1) + po_quote(fixed)
            new_lines.append(ln)
        out.append("\n".join(new_lines))
    with open(path, "w", encoding="utf-8", newline="") as f:
        f.write("\n\n".join(out))
    return changed


def placeholders(text):
    text = text.replace("{{", "").replace("}}", "")
    return sorted(set(re.findall(r"\{([A-Za-z0-9_]+)\}", text)))


# =======================================================================================
# Команды
# =======================================================================================

def group(hits):
    """{(ctx, key): {"kinds": set, "places": [(file, line)]}}"""
    out = {}
    for scope, h in hits:
        slot = out.setdefault((h.ctx, h.key), {"kinds": set(), "places": [], "scopes": set(), "plural": None})
        if h.plural:
            slot["plural"] = h.plural
        slot["kinds"].add(h.kind)
        slot["places"].append((h.file, h.line))
        slot["scopes"].add(scope)
    return out


def main():
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--scope", default="all", help="editor, hub, installer, modeler или all (по умолчанию)")
    ap.add_argument("--check", action="store_true", help="сверить с каталогом; код выхода 1 при расхождениях")
    ap.add_argument("--update", action="store_true", help="дописать недостающие ключи в каталог пустым msgstr")
    ap.add_argument("--stats", action="store_true", help="число мест по файлам")
    ap.add_argument("--json", action="store_true", help="список ключей в JSON")
    ap.add_argument("--catalogue", default=os.path.join(ROOT, CATALOGUE))
    ap.add_argument("--candidates-only", action="store_true", help="только ещё не обёрнутые")
    ap.add_argument("--migrate", action="store_true", help="привести ключи каталога к виду после wrap_ui_strings.py")
    args = ap.parse_args()

    if args.migrate:
        print("migrated", migrate_catalogue(args.catalogue), "strings")
        return 0
    scopes = list(SCOPES) if args.scope == "all" else [s.strip() for s in args.scope.split(",")]
    for s in scopes:
        if s not in SCOPES:
            ap.error(f"unknown scope {s!r}")
    hits = collect(scopes)
    keys = group(hits)

    if args.stats:
        per = {}
        for scope, h in hits:
            slot = per.setdefault(h.file, [0, 0])
            slot[0 if h.kind == "wrapped" else 1] += 1
        print(f"{'file':64} {'wrapped':>8} {'to wrap':>8}")
        for f in sorted(per):
            print(f"{f:64} {per[f][0]:8} {per[f][1]:8}")
        by_scope = {}
        for scope, h in hits:
            slot = by_scope.setdefault(scope, [0, 0, set()])
            slot[0 if h.kind == "wrapped" else 1] += 1
            slot[2].add((h.ctx, h.key))
        for scope, (w, c, k) in sorted(by_scope.items()):
            print(f"[{scope}] call sites: {w} wrapped, {c} to wrap; unique keys: {len(k)}")
        print(f"total unique keys: {len(keys)}")
        return 0

    if args.json:
        print(json.dumps([{"ctx": c, "key": k, "kinds": sorted(v["kinds"]), "places": [f"{f}:{l}" for f, l in v["places"]]}
                          for (c, k), v in sorted(keys.items(), key=lambda kv: (kv[0][1].lower(), kv[0][0] or ""))],
                         ensure_ascii=False, indent=1))
        return 0

    if not (args.check or args.update):
        for (ctx, key), v in sorted(keys.items(), key=lambda kv: (kv[0][1].lower(), kv[0][0] or "")):
            kind = "wrapped" if "wrapped" in v["kinds"] else "to-wrap"
            if args.candidates_only and kind == "wrapped":
                continue
            places = ", ".join(f"{f}:{l}" for f, l in v["places"][:3])
            more = f" (+{len(v['places']) - 3})" if len(v["places"]) > 3 else ""
            ctx_text = f"[{ctx}] " if ctx else ""
            print(f"{kind:8} {ctx_text}{key!r}  {places}{more}")
        print(f"\n{len(keys)} unique strings", file=sys.stderr)
        return 0

    entries, errors = load_catalogue(args.catalogue)
    problems = list(errors)
    missing = []
    for (ctx, key), v in sorted(keys.items(), key=lambda kv: (kv[0][1].lower(), kv[0][0] or "")):
        forms = entries.get((ctx, key))
        place = f"{v['places'][0][0]}:{v['places'][0][1]}"
        if v["plural"] and forms is not None and len(forms) != 3:
            problems.append(f"plural: {key!r} needs 3 forms in the catalogue, has {len(forms)}  ({place})")
            continue
        if forms is None or any(not f for f in forms):
            missing.append((ctx, key, v))
            problems.append(f"missing: {key!r}{' [' + ctx + ']' if ctx else ''}  ({place})")
        else:
            expected = placeholders(key)
            for f in forms:
                if placeholders(f) != expected and "wrapped" in v["kinds"] or (placeholders(f) != expected and forms):
                    problems.append(f"placeholders: {key!r} -> {f!r}  ({place})")
                    break
    if args.scope == "all":
        used = set(keys)
        for (ctx, key) in entries:
            if (ctx, key) not in used:
                problems.append(f"unused: {key!r}{' [' + ctx + ']' if ctx else ''}")

    if args.update:
        if not missing:
            print("catalogue is complete")
        else:
            with open(args.catalogue, "a", encoding="utf-8", newline="\n") as f:
                for ctx, key, v in missing:
                    if (ctx, key) in entries:
                        continue
                    f.write("\n")
                    for file, line in v["places"][:2]:
                        f.write(f"#: {file}:{line}\n")
                    if ctx:
                        f.write(f"msgctxt {po_quote(ctx)}\n")
                    f.write(f"msgid {po_quote(key)}\n")
                    if v["plural"]:
                        f.write(f"msgid_plural {po_quote(v['plural'])}\n")
                        f.write('msgstr[0] ""\nmsgstr[1] ""\nmsgstr[2] ""\n')
                    else:
                        f.write('msgstr ""\n')
            print(f"appended {len(missing)} entries with empty translations to {args.catalogue}")
        return 0

    for p in problems:
        print(p)
    wrapped = sum(1 for v in keys.values() if "wrapped" in v["kinds"])
    print(f"{len(keys)} strings ({wrapped} wrapped in code), catalogue has {len(entries)} entries, {len(problems)} problems")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

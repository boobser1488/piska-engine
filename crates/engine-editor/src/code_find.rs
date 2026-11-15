//! Поиск вкладки Code: учёт регистра, целые слова, регулярные выражения (свой небольшой
//! движок с возвратом — крейта `regex` в рабочем пространстве нет), замена с группами
//! `$1` / `${name}` нет — только номера, `$0` — всё совпадение) и с сохранением регистра.
//!
//! Синтаксис выражений: `.` `\d \w \s \D \W \S` `\b` `[a-z_]` `[^…]` `^` `$` (границы
//! строки) `( … )` `(?: … )` `|` `* + ? {n} {n,} {n,m}` и ленивые `*? +? ??`.

// Сопоставление с возвратом через продолжения: типы замыканий длинные по природе.
#![allow(clippy::type_complexity)]

use crate::i18n::{tr_s, trf};
use std::ops::Range;

/// Параметры поиска.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Замена: `speed` → `pace` даёт `Pace` для `Speed` и `PACE` для `SPEED`.
    pub preserve_case: bool,
}

#[derive(Clone, Debug)]
enum Class {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

#[derive(Clone, Debug)]
enum Node {
    Char(char),
    Any,
    Set(Vec<Class>, bool),
    Start,
    End,
    WordBoundary,
    Group(Box<Node>, Option<usize>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, usize, usize, bool),
}

/// Разобранное регулярное выражение.
#[derive(Clone, Debug)]
pub(crate) struct Regex {
    node: Node,
    groups: usize,
    ignore_case: bool,
}

struct Parser<'a> {
    chars: Vec<char>,
    i: usize,
    groups: usize,
    _text: &'a str,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn alt(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.concat()?];
        while self.peek() == Some('|') {
            self.i += 1;
            branches.push(self.concat()?);
        }
        Ok(if branches.len() == 1 { branches.pop().expect("one") } else { Node::Alt(branches) })
    }

    fn concat(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            items.push(self.quantifier(atom)?);
        }
        Ok(Node::Concat(items))
    }

    fn quantifier(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => (0, usize::MAX),
            Some('+') => (1, usize::MAX),
            Some('?') => (0, 1),
            Some('{') => {
                let close = self.chars[self.i..].iter().position(|c| *c == '}').map(|p| self.i + p);
                let Some(close) = close else { return Ok(atom) };
                let body: String = self.chars[self.i + 1..close].iter().collect();
                let parse = |s: &str| s.trim().parse::<usize>().ok();
                let (min, max) = match body.split_once(',') {
                    Some((a, b)) if b.trim().is_empty() => (parse(a), Some(usize::MAX)),
                    Some((a, b)) => (parse(a), parse(b)),
                    None => (parse(&body), parse(&body)),
                };
                let (Some(min), Some(max)) = (min, max) else { return Ok(atom) };
                if min > max {
                    return Err(tr_s("a repetition {n,m} needs n ≤ m").to_string());
                }
                self.i = close;
                (min, max)
            }
            _ => return Ok(atom),
        };
        self.i += 1;
        let greedy = if self.peek() == Some('?') {
            self.i += 1;
            false
        } else {
            true
        };
        if matches!(atom, Node::Start | Node::End | Node::WordBoundary) {
            return Err(tr_s("nothing to repeat").to_string());
        }
        Ok(Node::Repeat(Box::new(atom), min, max, greedy))
    }

    fn escape(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or_else(|| tr_s("`\\` at the end of the expression").to_string())?;
        self.i += 1;
        Ok(match c {
            'd' => Node::Set(vec![Class::Digit(true)], false),
            'D' => Node::Set(vec![Class::Digit(false)], false),
            'w' => Node::Set(vec![Class::Word(true)], false),
            'W' => Node::Set(vec![Class::Word(false)], false),
            's' => Node::Set(vec![Class::Space(true)], false),
            'S' => Node::Set(vec![Class::Space(false)], false),
            'b' => Node::WordBoundary,
            'n' => Node::Char('\n'),
            't' => Node::Char('\t'),
            other => Node::Char(other),
        })
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.peek().expect("checked by the caller");
        self.i += 1;
        match c {
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '\\' => self.escape(),
            '(' => {
                let capture = if self.chars[self.i..].starts_with(&['?', ':']) {
                    self.i += 2;
                    None
                } else {
                    self.groups += 1;
                    Some(self.groups)
                };
                let inner = self.alt()?;
                if self.peek() != Some(')') {
                    return Err(tr_s("missing `)`").to_string());
                }
                self.i += 1;
                Ok(Node::Group(Box::new(inner), capture))
            }
            ')' => Err(tr_s("unmatched `)`").to_string()),
            '[' => {
                let negated = self.peek() == Some('^');
                if negated {
                    self.i += 1;
                }
                let mut classes = Vec::new();
                let mut first = true;
                loop {
                    let Some(c) = self.peek() else { return Err(tr_s("missing `]`").to_string()) };
                    self.i += 1;
                    if c == ']' && !first {
                        break;
                    }
                    first = false;
                    let lo = if c == '\\' {
                        match self.escape()? {
                            Node::Char(ch) => ch,
                            Node::Set(mut set, _) => {
                                classes.append(&mut set);
                                continue;
                            }
                            _ => continue,
                        }
                    } else {
                        c
                    };
                    if self.peek() == Some('-') && self.chars.get(self.i + 1).is_some_and(|n| *n != ']') {
                        let hi = self.chars[self.i + 1];
                        self.i += 2;
                        classes.push(Class::Range(lo, hi));
                    } else {
                        classes.push(Class::Range(lo, lo));
                    }
                }
                Ok(Node::Set(classes, negated))
            }
            '*' | '+' | '?' => Err(tr_s("nothing to repeat").to_string()),
            other => Ok(Node::Char(other)),
        }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Предел шагов возврата: выражение вида `(a*)*b` не повесит редактор.
const STEP_LIMIT: usize = 2_000_000;

struct Matcher<'a> {
    text: &'a [char],
    ignore_case: bool,
    steps: usize,
}

impl Matcher<'_> {
    fn eq(&self, a: char, b: char) -> bool {
        a == b || (self.ignore_case && a.to_lowercase().eq(b.to_lowercase()))
    }

    fn class(&self, classes: &[Class], negated: bool, c: char) -> bool {
        let hit = classes.iter().any(|class| match class {
            Class::Range(lo, hi) => {
                (*lo <= c && c <= *hi)
                    || (self.ignore_case && c.to_lowercase().chain(c.to_uppercase()).any(|x| *lo <= x && x <= *hi))
            }
            Class::Digit(yes) => c.is_ascii_digit() == *yes,
            Class::Word(yes) => is_word(c) == *yes,
            Class::Space(yes) => c.is_whitespace() == *yes,
        });
        hit != negated
    }

    /// Сопоставить `node` с позиции `i`; при успехе вызвать `k` с концом. CPS с возвратом.
    fn m(&mut self, node: &Node, i: usize, caps: &mut Vec<Option<(usize, usize)>>, k: &mut dyn FnMut(&mut Self, usize, &mut Vec<Option<(usize, usize)>>) -> bool) -> bool {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            return false;
        }
        match node {
            Node::Char(c) => i < self.text.len() && self.eq(self.text[i], *c) && k(self, i + 1, caps),
            Node::Any => i < self.text.len() && self.text[i] != '\n' && k(self, i + 1, caps),
            Node::Set(classes, negated) => i < self.text.len() && self.class(classes, *negated, self.text[i]) && k(self, i + 1, caps),
            Node::Start => (i == 0 || self.text[i - 1] == '\n') && k(self, i, caps),
            Node::End => (i == self.text.len() || self.text[i] == '\n') && k(self, i, caps),
            Node::WordBoundary => {
                let before = i > 0 && is_word(self.text[i - 1]);
                let after = i < self.text.len() && is_word(self.text[i]);
                before != after && k(self, i, caps)
            }
            Node::Group(inner, capture) => {
                let capture = *capture;
                self.m(inner, i, caps, &mut |me: &mut Self, end, caps: &mut Vec<Option<(usize, usize)>>| {
                    let saved = capture.map(|g| caps[g]);
                    if let Some(g) = capture {
                        caps[g] = Some((i, end));
                    }
                    if k(me, end, caps) {
                        return true;
                    }
                    if let (Some(g), Some(saved)) = (capture, saved) {
                        caps[g] = saved;
                    }
                    false
                })
            }
            Node::Concat(items) => self.seq(items, i, caps, k),
            Node::Alt(branches) => branches.iter().any(|b| self.m(b, i, caps, k)),
            Node::Repeat(inner, min, max, greedy) => self.repeat(inner, *min, *max, *greedy, 0, i, caps, k),
        }
    }

    fn seq(&mut self, items: &[Node], i: usize, caps: &mut Vec<Option<(usize, usize)>>, k: &mut dyn FnMut(&mut Self, usize, &mut Vec<Option<(usize, usize)>>) -> bool) -> bool {
        match items.split_first() {
            None => k(self, i, caps),
            Some((first, rest)) => self.m(first, i, caps, &mut |me: &mut Self, j, caps: &mut Vec<Option<(usize, usize)>>| me.seq(rest, j, caps, k)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat(&mut self, inner: &Node, min: usize, max: usize, greedy: bool, count: usize, i: usize, caps: &mut Vec<Option<(usize, usize)>>, k: &mut dyn FnMut(&mut Self, usize, &mut Vec<Option<(usize, usize)>>) -> bool) -> bool {
        if count < min {
            return self.repeat_more(inner, min, max, greedy, count, i, caps, k);
        }
        // Жадное повторение пробует ещё одно раньше остатка, ленивое — позже: порядок важен.
        #[allow(clippy::if_same_then_else)]
        if greedy {
            self.repeat_more(inner, min, max, greedy, count, i, caps, k) || k(self, i, caps)
        } else {
            k(self, i, caps) || self.repeat_more(inner, min, max, greedy, count, i, caps, k)
        }
    }

    /// Ещё одно повторение `inner` и остаток повторений.
    #[allow(clippy::too_many_arguments)]
    fn repeat_more(&mut self, inner: &Node, min: usize, max: usize, greedy: bool, count: usize, i: usize, caps: &mut Vec<Option<(usize, usize)>>, k: &mut dyn FnMut(&mut Self, usize, &mut Vec<Option<(usize, usize)>>) -> bool) -> bool {
        count < max
            && self.m(inner, i, caps, &mut |me: &mut Self, j, caps: &mut Vec<Option<(usize, usize)>>| {
                // Пустое повторение не зацикливается.
                j > i && me.repeat(inner, min, max, greedy, count + 1, j, caps, k)
            })
    }
}

impl Regex {
    pub fn new(pattern: &str, ignore_case: bool) -> Result<Self, String> {
        let mut parser = Parser { chars: pattern.chars().collect(), i: 0, groups: 0, _text: pattern };
        let node = parser.alt()?;
        if parser.i < parser.chars.len() {
            return Err(tr_s("unmatched `)`").to_string());
        }
        Ok(Self { node, groups: parser.groups, ignore_case })
    }

    /// Все непересекающиеся совпадения: диапазоны символов и группы.
    fn find_chars(&self, chars: &[char]) -> Vec<Vec<Option<(usize, usize)>>> {
        let mut out = Vec::new();
        let mut matcher = Matcher { text: chars, ignore_case: self.ignore_case, steps: 0 };
        let mut start = 0;
        while start <= chars.len() {
            let mut caps = vec![None; self.groups + 1];
            let mut found = None;
            let ok = matcher.m(&self.node, start, &mut caps, &mut |_, end, caps: &mut Vec<Option<(usize, usize)>>| {
                found = Some((end, caps.clone()));
                true
            });
            if matcher.steps > STEP_LIMIT {
                break;
            }
            match (ok, found) {
                (true, Some((end, mut groups))) => {
                    groups[0] = Some((start, end));
                    out.push(groups);
                    start = if end > start { end } else { start + 1 };
                }
                _ => start += 1,
            }
        }
        out
    }
}

/// Совпадение: байтовый диапазон и группы (байтовые диапазоны; `None` — группа не участвовала).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Match {
    pub range: Range<usize>,
    pub groups: Vec<Option<Range<usize>>>,
}

/// Найти все совпадения `query` в `text`. `Err` — ошибка в регулярном выражении.
pub(crate) fn find_all(text: &str, query: &str, options: FindOptions) -> Result<Vec<Match>, String> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if options.regex {
        let regex = Regex::new(query, !options.match_case).map_err(|e| trf!("regular expression: {e}", e = e))?;
        let chars: Vec<char> = text.chars().collect();
        let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        offsets.push(text.len());
        for groups in regex.find_chars(&chars) {
            let to_bytes = |g: &Option<(usize, usize)>| g.map(|(a, b)| offsets[a]..offsets[b]);
            let range = to_bytes(&groups[0]).expect("group 0");
            if options.whole_word && !whole(text, &range) {
                continue;
            }
            out.push(Match { range, groups: groups.iter().map(to_bytes).collect() });
        }
        return Ok(out);
    }
    let (hay, needle) = if options.match_case { (text.to_string(), query.to_string()) } else { (text.to_lowercase(), query.to_lowercase()) };
    // `to_lowercase` может менять длину (редкие символы) — тогда ищем без него.
    let (hay, needle) = if hay.len() == text.len() { (hay, needle) } else { (text.to_string(), query.to_string()) };
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        if text.is_char_boundary(start) && text.is_char_boundary(end) && (!options.whole_word || whole(text, &(start..end))) {
            out.push(Match { range: start..end, groups: vec![Some(start..end)] });
        }
        from = crate::code_diagnostics::ceil_char(text, start + 1);
        if from >= text.len() {
            break;
        }
    }
    Ok(out)
}

fn whole(text: &str, range: &Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back().is_some_and(is_word);
    let after = text[range.end..].chars().next().is_some_and(is_word);
    !before && !after
}

/// Текст замены одного совпадения: `$0`…`$9` из регулярного выражения, сохранение регистра.
pub(crate) fn replacement(text: &str, m: &Match, replace: &str, options: FindOptions) -> String {
    let mut out = String::new();
    if options.regex {
        let mut chars = replace.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '$' {
                if let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                    chars.next();
                    if let Some(Some(g)) = m.groups.get(d as usize) {
                        out.push_str(&text[g.clone()]);
                    }
                    continue;
                }
                if chars.peek() == Some(&'$') {
                    chars.next();
                }
            }
            out.push(c);
        }
    } else {
        out.push_str(replace);
    }
    if options.preserve_case {
        let found = &text[m.range.clone()];
        if found.chars().any(char::is_alphabetic) && found.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) && found.chars().filter(|c| c.is_alphabetic()).count() > 1 {
            return out.to_uppercase();
        }
        if found.chars().next().is_some_and(char::is_uppercase) {
            let mut chars = out.chars();
            if let Some(first) = chars.next() {
                return first.to_uppercase().collect::<String>() + chars.as_str();
            }
        }
    }
    out
}

/// Заменить все совпадения.
pub(crate) fn replace_all(text: &str, matches: &[Match], replace: &str, options: FindOptions) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in matches {
        out.push_str(&text[last..m.range.start]);
        out.push_str(&replacement(text, m, replace, options));
        last = m.range.end;
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges<'a>(text: &'a str, query: &str, options: FindOptions) -> Vec<&'a str> {
        find_all(text, query, options).unwrap().into_iter().map(|m| &text[m.range]).collect::<Vec<_>>()
    }

    #[test]
    fn plain_search_options() {
        let text = "speed Speed speedy SPEED";
        assert_eq!(ranges(text, "speed", FindOptions::default()).len(), 4);
        assert_eq!(ranges(text, "speed", FindOptions { match_case: true, ..Default::default() }), ["speed", "speed"]);
        assert_eq!(ranges(text, "speed", FindOptions { whole_word: true, ..Default::default() }), ["speed", "Speed", "SPEED"]);
    }

    #[test]
    fn regex_engine() {
        let r = FindOptions { regex: true, match_case: true, ..Default::default() };
        assert_eq!(ranges("a1 b22 c333", r"\d+", r), ["1", "22", "333"]);
        assert_eq!(ranges("fn go() fn stop()", r"fn (\w+)\(", r), ["fn go(", "fn stop("]);
        assert_eq!(ranges("x = 10;\ny = 2;", r"^\w", r), ["x", "y"]);
        assert_eq!(ranges("x = 10;\ny = 2;", r";$", r), [";", ";"]);
        assert_eq!(ranges("cat dog cow", "c(a|o)w|dog", r), ["dog", "cow"]);
        assert_eq!(ranges("aaa", "a{2}", r), ["aa"]);
        assert_eq!(ranges("<a><b>", "<.+?>", r), ["<a>", "<b>"]);
        assert_eq!(ranges("<a><b>", "<.+>", r), ["<a><b>"]);
        assert_eq!(ranges("x_1 y-2", "[a-z_]+[0-9]", r), ["x_1"]);
        assert_eq!(ranges("ABC", "[^B]", r), ["A", "C"]);
        assert_eq!(ranges("Привет мир", r"\bмир\b", r), ["мир"]);
        assert_eq!(ranges("SPEED", "speed", FindOptions { regex: true, ..Default::default() }), ["SPEED"]);
        assert!(find_all("x", "(a", r).is_err());
        assert!(find_all("x", "*a", r).is_err());
        // Катастрофический возврат обрывается, а не вешает.
        let started = std::time::Instant::now();
        let _ = find_all(&"a".repeat(40), "(a*)*b", r);
        assert!(started.elapsed().as_secs_f32() < 5.0);
    }

    #[test]
    fn replace_with_groups_and_preserved_case() {
        let r = FindOptions { regex: true, match_case: true, ..Default::default() };
        let text = "let speed = 1; let jump = 2;";
        let m = find_all(text, r"let (\w+) = (\d)", r).unwrap();
        assert_eq!(replace_all(text, &m, "const $1: f32 = $2.0", r), "const speed: f32 = 1.0; const jump: f32 = 2.0;");
        let p = FindOptions { preserve_case: true, ..Default::default() };
        let text = "speed Speed SPEED";
        let m = find_all(text, "speed", p).unwrap();
        assert_eq!(replace_all(text, &m, "pace", p), "pace Pace PACE");
    }
}

//! Правила `.gitignore`: подмножество, которого хватает проектам, — комментарии, `!`,
//! `/` в начале и в конце, `*`, `?`, `[abc]`, `**`. Каждый `.gitignore` действует на свой
//! каталог и глубже; более глубокий файл главнее.

/// Одно правило.
#[derive(Clone, Debug)]
struct Rule {
    segments: Vec<String>,
    negate: bool,
    dir_only: bool,
    /// Есть `/` в начале или в середине: сопоставляется с путём от каталога файла правил,
    /// иначе — с именем на любой глубине.
    anchored: bool,
}

/// Правила одного файла `.gitignore`.
#[derive(Clone, Debug, Default)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    pub fn parse(text: &str) -> Self {
        let mut rules = Vec::new();
        for raw in text.lines() {
            let mut line = raw.trim_end_matches('\r');
            // Пробелы в конце отбрасываются, если не экранированы.
            while line.ends_with(' ') && !line.ends_with("\\ ") {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negate, line) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let (dir_only, line) = match line.strip_suffix('/') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            if line.is_empty() {
                continue;
            }
            let anchored = line.contains('/');
            let line = line.strip_prefix('/').unwrap_or(line);
            let segments: Vec<String> = line.split('/').map(str::to_string).collect();
            rules.push(Rule {
                segments,
                negate,
                dir_only,
                anchored,
            });
        }
        Self { rules }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Что решают правила о пути `relative` (от каталога файла правил, через `/`):
    /// `Some(true)` — игнорировать, `Some(false)` — вернуть (`!`), `None` — не упомянут.
    pub fn verdict(&self, relative: &str, is_dir: bool) -> Option<bool> {
        let parts: Vec<&str> = relative.split('/').collect();
        let mut result = None;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let hit = if rule.anchored {
                match_segments(&rule.segments, &parts)
            } else {
                parts.last().is_some_and(|name| match_segments(&rule.segments, &[name]))
            };
            if hit {
                result = Some(!rule.negate);
            }
        }
        result
    }
}

fn match_segments(pattern: &[String], text: &[&str]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((first, rest)) if first == "**" => {
            // `**` — ноль или больше компонентов; в конце — всё внутри (минимум один).
            if rest.is_empty() {
                return !text.is_empty();
            }
            (0..=text.len()).any(|skip| match_segments(rest, &text[skip..]))
        }
        Some((first, rest)) => match text.split_first() {
            Some((name, tail)) => wildcard(first.as_bytes(), name.as_bytes()) && match_segments(rest, tail),
            None => false,
        },
    }
}

/// `*`, `?`, `[...]` внутри одного компонента пути.
fn wildcard(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| wildcard(rest, &text[skip..])),
        Some((b'?', rest)) => !text.is_empty() && wildcard(rest, &text[1..]),
        Some((b'[', rest)) => {
            let Some(end) = rest.iter().position(|&c| c == b']').filter(|&e| e > 0) else {
                return !text.is_empty() && text[0] == b'[' && wildcard(rest, &text[1..]);
            };
            let (class, after) = (&rest[..end], &rest[end + 1..]);
            let Some((&c, tail)) = text.split_first() else {
                return false;
            };
            let (negated, class) = match class.first() {
                Some(b'!') | Some(b'^') => (true, &class[1..]),
                _ => (false, class),
            };
            let mut found = false;
            let mut i = 0;
            while i < class.len() {
                if i + 2 < class.len() && class[i + 1] == b'-' {
                    found |= (class[i]..=class[i + 2]).contains(&c);
                    i += 3;
                } else {
                    found |= class[i] == c;
                    i += 1;
                }
            }
            found != negated && wildcard(after, tail)
        }
        Some((b'\\', rest)) if !rest.is_empty() => !text.is_empty() && text[0] == rest[0] && wildcard(&rest[1..], &text[1..]),
        Some((&c, rest)) => !text.is_empty() && text[0] == c && wildcard(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignored(rules: &str, path: &str, dir: bool) -> bool {
        Rules::parse(rules).verdict(path, dir).unwrap_or(false)
    }

    #[test]
    fn gitignore_patterns() {
        assert!(ignored("/target\n", "target", true));
        assert!(!ignored("/target\n", "src/target", true), "anchored");
        assert!(ignored("*.log\n", "a/b/x.log", false));
        assert!(ignored("build/\n", "cpp/build", true));
        assert!(!ignored("build/\n", "cpp/build", false), "directory only");
        assert!(ignored("assets/**/*.tmp\n", "assets/a/b/c.tmp", false));
        assert!(ignored("assets/**/*.tmp\n", "assets/c.tmp", false));
        assert!(ignored("**/cache\n", "x/y/cache", true));
        assert!(ignored("doc/*.html\n", "doc/a.html", false));
        assert!(!ignored("doc/*.html\n", "doc/sub/a.html", false), "* stops at /");
        assert!(ignored("file?.txt\n", "file1.txt", false));
        assert!(ignored("[ab]x\n", "bx", false));
        assert!(!ignored("[!ab]x\n", "bx", false));
        assert!(ignored("# comment\n\n*.o\n", "a.o", false));
        // Отрицание главнее более раннего правила.
        let rules = Rules::parse("*.png\n!keep.png\n");
        assert_eq!(rules.verdict("a.png", false), Some(true));
        assert_eq!(rules.verdict("keep.png", false), Some(false));
        assert_eq!(rules.verdict("a.txt", false), None);
        assert!(ignored("crlf\r\n", "crlf", false));
    }
}

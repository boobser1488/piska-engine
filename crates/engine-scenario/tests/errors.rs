//! Каталог ошибок: сообщение, место `file:line:col`, выдержка, подсказка, стек.
//! Ни одна ошибка не паникует; пределы защищают от бесконечных циклов и рекурсии.

use engine_scenario::{ErrorKind, Group, Limits, Script, ScriptError, Value, Vm};

fn error_of(src: &str) -> ScriptError {
    let mut vm = Vm::new();
    match vm.run_source("errors.scenario", src) {
        Ok(v) => panic!("expected an error, got {v:?} from\n{src}"),
        Err(e) => e,
    }
}

/// (исходник, кусок сообщения, строка, столбец, кусок подсказки или "")
const CATALOGUE: &[(&str, &str, u32, u32, &str)] = &[
    // Незакрытые блоки.
    ("function f()\n  print(1)\n", "expected `end` to close the `function` opened at line 1", 3, 1, ""),
    ("if x then\n  y()\n", "expected `end` or `else` or `elseif` to close the `if` opened at line 1", 3, 1, ""),
    ("while true do\n", "to close the `while` opened at line 1", 2, 1, ""),
    ("function f() {\n  let a = 1\n", "expected `}` to close the `{` opened at line 1", 3, 1, ""),
    ("let l = [1, 2\n", "expected `]` to close the `[` opened at line 1", 2, 1, ""),
    ("print((1 + 2)\n", "expected `)` to close the `(` opened at line 1", 2, 1, ""),
    ("function f() {\n  return 1\nend", "close it with `}`", 3, 1, ""),
    ("let s = \"abc\nprint(s)", "this string is never closed", 1, 9, ""),
    ("/* never closed", "never closed with `*/`", 1, 1, ""),
    ("let t = `abc ${x", "is never closed", 1, 16, ""),
    // Имена и объявления.
    ("let score = 1\nprint(scroe)", "`scroe` is not defined", 2, 7, "did you mean `score`?"),
    ("prnt(\"hi\")", "`prnt` is not defined", 1, 1, "did you mean `print`?"),
    ("hp = 10", "`hp` is not declared", 1, 1, ""),
    ("const MAX = 3\nMAX = 4", "`MAX` is a const and can't be changed", 2, 1, ""),
    ("function f() { const k = 1; k += 1 }", "`k` is a const", 1, 29, ""),
    ("let a = 1\nlet a = 2", "`a` is already declared on line 1", 2, 5, ""),
    ("function f() {\n  print(x)\n  let x = 1\n}", "`x` is used before its declaration on line 3", 2, 9, ""),
    // Синтаксис.
    ("let = 5", "expected a variable name, found `=`", 1, 5, ""),
    ("let end = 5", "keywords can't be used as names", 1, 5, ""),
    ("x +", "expected an expression", 1, 4, ""),
    ("let i = 0\ni--", "this line computes a value but does nothing", 2, 1, "`--` starts a comment"),
    ("let a = 1\na == 2", "does nothing", 2, 1, "single `=`"),
    ("break", "`break` can only be used inside a loop", 1, 1, ""),
    ("let x = 1 & 2", "use `&&` or `and`", 1, 11, ""),
    ("let m = {a = 1, 2}", "can't mix `key = value` entries with plain items", 1, 17, ""),
    ("let n = 12abc", "malformed number", 1, 9, ""),
    ("if (x) { } elseif (y) { }", "`elseif` belongs to the Lua form", 1, 12, ""),
    // Выполнение.
    ("let player = nil\nplayer.hp = 3", "tried to set `hp` on nil — `player` is nil", 2, 8, ""),
    ("let player = nil\nprint(player.hp)", "tried to read `.hp` of nil — `player` is nil", 2, 14, ""),
    ("let enemy = {}\nenemy.attack()", "the map (`enemy`) has no key or method `attack`", 2, 7, ""),
    ("let f = nil\nf()", "tried to call `f`, but it is nil", 2, 1, ""),
    ("let t = {}\nt.go()", "has no key or method `go`", 2, 3, ""),
    ("let x = 1 + nil", "can't add number and nil: the right side is nil", 1, 11, ""),
    ("let x = {} * 2", "can't multiply map and number", 1, 12, ""),
    ("let x = 'a' < 1", "can't compare the string \"a\" with the number 1", 1, 13, ""),
    ("math.sqrt('x')", "bad argument #1 to `sqrt`: expected a number, got the string \"x\"", 1, 6, ""),
    ("let l = [1]\nl.push2(3)", "list (`l`) has no method `push2`", 2, 3, "did you mean `push`?"),
    ("[1, 2].map(5)", "bad argument to `map`: expected a function, got the number 5", 1, 8, ""),
    ("error('custom failure')", "custom failure", 1, 1, ""),
    ("assert(1 > 2, 'math is broken')", "math is broken", 1, 1, ""),
    ("let l = [1]\nl[5] = 2", "index 5 is out of range for a list of length 1", 2, 2, ""),
    ("for (const x of 5) { }", "can't loop over the number 5", 1, 17, ""),
    ("let v = vec2(1, 2)\nprint(v.z)", "vec2 (`v`) has no field `z`", 2, 9, ""),
    ("const p = vec2(1, 2)\np.x = 5", "`p` is a const; vectors and colors are values", 2, 1, ""),
    ("let s = 'abc'\ns[0] = 'x'", "strings can't be changed in place", 2, 2, ""),
    ("print(#nil)", "can't take the length of nil", 1, 7, ""),
    ("for i = 1, 'x' do end", "a numeric `for` needs numbers", 1, 1, ""),
    ("let m = {}\nm[nil] = 1", "a map key must be a string, number, boolean or object, not nil", 2, 2, ""),
];

#[test]
fn error_catalogue_messages_and_positions() {
    let mut failures = Vec::new();
    for (src, msg, line, col, hint) in CATALOGUE {
        let e = error_of(src);
        let ok = e.message.contains(msg)
            && e.pos.line == *line
            && e.pos.col == *col
            && (hint.is_empty() || e.hint.as_deref().unwrap_or("").contains(hint));
        if !ok {
            failures.push(format!(
                "source: {src:?}\n  want: {line}:{col} {msg:?} hint {hint:?}\n  got:  {}:{} {:?} hint {:?}",
                e.pos.line, e.pos.col, e.message, e.hint
            ));
        }
        assert_eq!(&*e.file, "errors.scenario");
    }
    assert!(failures.is_empty(), "{} of {} cases failed:\n{}", failures.len(), CATALOGUE.len(), failures.join("\n"));
}

#[test]
fn report_has_location_excerpt_caret_and_stack() {
    let src = "function inner(x) {\n  return x.value\n}\nfunction outer() {\n  return inner(nil)\n}\nouter()";
    let e = error_of(src);
    assert_eq!(e.kind, ErrorKind::Runtime);
    assert_eq!(e.location(), "errors.scenario:2:12");
    let text = e.report();
    assert!(text.starts_with("errors.scenario:2:12: tried to read `.value` of nil — `x` is nil"), "{text}");
    assert!(text.contains(" 2 |   return x.value\n   |            ^"), "{text}");
    let frames: Vec<_> = e.stack.iter().map(|f| (f.function.as_str(), f.pos.line)).collect();
    assert_eq!(frames, [("inner", 2), ("outer", 5), ("<main>", 7)]);
}

#[test]
fn errors_inside_library_callbacks_point_at_the_user_code() {
    let e = error_of("let r = [1, 2].map((x) => x.y)");
    assert_eq!((e.pos.line, e.pos.col), (1, 29), "{e}");
    assert!(e.stack.iter().all(|f| !f.function.starts_with("__")), "{e}");
}

#[test]
fn a_failing_task_does_not_stop_the_others() {
    let mut vm = Vm::new();
    let src = "start(() => { wait_frame()\nlet x = nil\nx.boom() })\nlet n = 0\nwhile (true) { n++\nprint(n)\nwait_frame() }";
    let script = Script::compile("t.scenario", src).unwrap();
    let mut g = Group::new(0);
    g.start(&vm, &script).unwrap();
    let mut errors = Vec::new();
    for _ in 0..4 {
        errors.extend(g.update(&mut vm, &mut (), 0.1, &mut |_, _| None).errors);
    }
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].pos.line, 3);
    assert_eq!(vm.take_output(), ["1", "2", "3", "4"]);
}

#[test]
fn infinite_loop_is_preempted_then_stopped() {
    let mut vm = Vm::new();
    vm.limits = Limits { instructions_per_resume: 10_000, max_busy_frames: 5, ..Limits::default() };
    let script = Script::compile("loop.scenario", "let n = 0\nwhile (true) {\n  n += 1\n}").unwrap();
    let mut g = Group::new(0);
    g.start(&vm, &script).unwrap();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    for _ in 0..10 {
        let r = g.update(&mut vm, &mut (), 0.1, &mut |_, _| None);
        warnings.extend(r.warnings);
        errors.extend(r.errors);
    }
    assert_eq!(warnings.len(), 1, "one warning per script: {warnings:?}");
    assert!(warnings[0].contains("without waiting"));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, ErrorKind::Limit);
    assert!(errors[0].message.contains("frames without waiting"), "{}", errors[0]);
    assert!((2..=3).contains(&errors[0].pos.line), "{}", errors[0]);
    assert!(g.is_idle());
    // Цикл с ожиданием — не нарушение.
    let script = Script::compile("ok.scenario", "while (true) { wait_frame() }").unwrap();
    let mut g = Group::new(0);
    g.start(&vm, &script).unwrap();
    for _ in 0..20 {
        let r = g.update(&mut vm, &mut (), 0.1, &mut |_, _| None);
        assert!(r.errors.is_empty() && r.warnings.is_empty());
    }
}

#[test]
fn a_long_computation_spans_frames_and_finishes() {
    let mut vm = Vm::new();
    vm.limits.instructions_per_resume = 50_000;
    let script = Script::compile("long.scenario", "let s = 0\nfor (let i = 0; i < 100000; i++) { s += i }\nprint(s)").unwrap();
    let mut g = Group::new(0);
    g.start(&vm, &script).unwrap();
    let mut frames = 0;
    while !g.is_idle() && frames < 1000 {
        let r = g.update(&mut vm, &mut (), 0.1, &mut |_, _| None);
        assert!(r.errors.is_empty());
        frames += 1;
    }
    assert!(frames > 5, "should have been spread over several frames, took {frames}");
    assert_eq!(vm.take_output(), ["4999950000"]);
}

#[test]
fn deep_recursion_is_an_error_not_a_crash() {
    let e = error_of("function down(n) { return down(n + 1) }\ndown(0)");
    assert_eq!(e.kind, ErrorKind::Limit);
    assert!(e.message.contains("too much recursion"), "{e}");
    assert!(e.stack.len() <= 10);
    // try его не ловит: иначе бесконечная рекурсия внутри try повторялась бы.
    let e = error_of("function down(n) { return down(n + 1) }\ntry { down(0) } catch (e) { print('caught') }");
    assert_eq!(e.kind, ErrorKind::Limit);
}

#[test]
fn memory_and_string_limits() {
    let mut vm = Vm::new();
    vm.limits.max_values = 50_000;
    let e = vm.run_source("mem.scenario", "let l = []\nwhile (true) { l.push(1) }").unwrap_err();
    assert_eq!(e.kind, ErrorKind::Limit);
    assert!(e.message.contains("out of script memory"), "{e}");
    let mut vm = Vm::new();
    vm.limits.max_string_len = 1000;
    let e = vm.run_source("str.scenario", "let s = 'x'\nwhile (true) { s = s + s }").unwrap_err();
    assert!(e.message.contains("string too long"), "{e}");
    // После ошибки память свободна, ВМ работает дальше.
    assert_eq!(vm.run_source("ok.scenario", "return [1, 2].length").unwrap(), Value::Num(2.0));
}

#[test]
fn deeply_nested_source_is_an_error_not_a_crash() {
    let deep = format!("let x = {}1{}", "(".repeat(5000), ")".repeat(5000));
    let e = error_of(&deep);
    assert!(e.message.contains("nested too deeply"), "{e}");
    let deep = format!("{}{}", "if true then\n".repeat(400), "end\n".repeat(400));
    let e = error_of(&deep);
    assert!(e.message.contains("nested too deeply"), "{e}");
    let deep = format!("let x = {}1", "-".repeat(3000));
    // `--` начинает комментарий, так что здесь просто незаконченное выражение.
    let _ = Vm::new().run_source("d.scenario", &deep);
    let deep = format!("let x = {}1", "!".repeat(3000));
    let e = error_of(&format!("{deep}\nx()"));
    assert!(e.message.contains("nested too deeply"), "{e}");
}

#[test]
fn deeply_nested_values_drop_without_overflow() {
    let mut vm = Vm::new();
    let v = vm.run_source("deep.scenario", "let a = []\nfor (let i = 0; i < 200000; i++) { a = [a] }\nreturn 1").unwrap();
    assert_eq!(v, Value::Num(1.0));
    let v = vm.run_source("deep2.scenario", "let a = {}\nfor (let i = 0; i < 200000; i++) { a = {next: a} }\nreturn tostring(a).length > 0").unwrap();
    assert_eq!(v, Value::Bool(true));
}

/// Детерминированный генератор для «fuzz»-теста.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

const CORPUS: &[&str] = &[
    "function greet(who) { return `Привет, ${who}` }\nlet l = [1, 2, 3].map((x) => x * 2)\nfor (const [k, v] of {a: 1}) print(k, v)\n",
    "local t = { name = 'a', [1] = 2 }\nfor i = 10, 1, -1 do if i % 2 == 0 then t[i] = i elseif i == 3 then break else continue end end\nrepeat x = 1 until true\n",
    "try { error({code: 1}) } catch (e) { print(e.code) }\nlet s = 'abc'.sub(0, 2) .. #[1]\nwhile (false) { }\ndo { } while (false)\n",
    "start(() => { wait(0.1); emit('x', 2) })\non('x', (v) => print(v))\nwait_until(() => true)\nlet v = vec3(1, 2, 3)\nv.x += 1\n",
];

#[test]
fn random_bytes_and_truncated_programs_never_panic() {
    let mut rng = Lcg(12345);
    let mut vm = Vm::new();
    vm.limits.instructions_per_resume = 20_000;
    vm.limits.max_busy_frames = 3;
    let mut compiled = 0;
    // Обрезанные программы.
    for src in CORPUS {
        let chars: Vec<char> = src.chars().collect();
        for cut in 0..chars.len() {
            let s: String = chars[..cut].iter().collect();
            if let Ok(script) = Script::compile("cut.scenario", &s) {
                compiled += 1;
                let _ = vm.run_script(&script, &mut (), 0.05, 5);
            }
        }
    }
    // Случайные байты и случайные склейки токенов.
    let pieces = [
        "let", "x", "=", "(", ")", "{", "}", "[", "]", "function", "end", "if", "then", "else", "for", "in", "of", "1", "'s'",
        "`t${", "}`", "..", "+", "-", "*", "/", "%", "^", "#", "!", "&&", "||", "and", "or", "not", ",", ";", ":", "?", "??",
        "=>", "...", "\n", "return", "while", "do", "repeat", "until", "try", "catch", "throw", "nil", "true", ".", "x.y",
        "wait(0)", "start(", "print(", "--", "//", "/*", "*/", "~=", "==", "++", "+=", "local", "const", "break", "continue",
    ];
    for _ in 0..3000 {
        let n = (rng.next() % 40) as usize;
        let mut s = String::new();
        if rng.next().is_multiple_of(4) {
            for _ in 0..n * 3 {
                s.push(char::from_u32((rng.next() % 0x500) as u32).unwrap_or('?'));
            }
        } else {
            for _ in 0..n {
                s.push_str(pieces[(rng.next() as usize) % pieces.len()]);
                s.push(' ');
            }
        }
        if let Ok(script) = Script::compile("fuzz.scenario", &s) {
            compiled += 1;
            let _ = vm.run_script(&script, &mut (), 0.05, 5);
        }
    }
    assert!(compiled > 50, "some random programs should compile, got {compiled}");
    // ВМ после всего этого исправна.
    assert_eq!(vm.run_source("ok.scenario", "return 2 + 2").unwrap(), Value::Num(4.0));
}

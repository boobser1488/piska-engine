//! Язык Scenario: обе формы записи дают одинаковый результат, приоритеты операторов,
//! замыкания, стандартная библиотека.

use engine_scenario::{Value, Vm};

fn run(src: &str) -> Value {
    let mut vm = Vm::new();
    match vm.run_source("test.scenario", src) {
        Ok(v) => v,
        Err(e) => panic!("script failed:\n{e}\n--- source ---\n{src}"),
    }
}

fn text(src: &str) -> String {
    run(src).to_text()
}

/// Каждая пара — одна и та же программа в форме Lua и в форме JavaScript.
const PAIRS: &[(&str, &str, &str)] = &[
    (
        "declarations",
        "local a = 1\nlocal name = \"Герой\"\nreturn name .. a",
        "let a = 1; const name = 'Герой'; var b = 0\nreturn name + a",
    ),
    (
        "function",
        "function greet(who)\n  return \"Привет, \" .. who\nend\nreturn greet(\"мир\")",
        "function greet(who) {\n  return \"Привет, \" + who\n}\nreturn greet(\"мир\")",
    ),
    (
        "anonymous function",
        "local double = function(x) return x * 2 end\nreturn double(21)",
        "const double = (x) => x * 2\nreturn double(21)",
    ),
    (
        "if chain",
        "local hp = 15\nlocal r\nif hp <= 0 then r = \"die\" elseif hp < 20 then r = \"warn\" else r = \"ok\" end\nreturn r",
        "let hp = 15\nlet r\nif (hp <= 0) { r = \"die\" } else if (hp < 20) { r = \"warn\" } else { r = \"ok\" }\nreturn r",
    ),
    (
        "while",
        "local i = 0\nwhile i < 10 do i = i + 1 end\nreturn i",
        "let i = 0\nwhile (i < 10) { i += 1 }\nreturn i",
    ),
    (
        "numeric for",
        "local s = 0\nfor i = 0, 9 do s = s + i end\nreturn s",
        "let s = 0\nfor (let i = 0; i < 10; i += 1) { s += i }\nreturn s",
    ),
    (
        "for of list",
        "local s = \"\"\nfor i, item in ipairs({\"a\", \"b\", \"c\"}) do s = s .. i .. item end\nreturn s",
        "let s = \"\"\nlet list = [\"a\", \"b\", \"c\"]\nfor (let i = 0; i < list.length; i++) { s += i + list[i] }\nreturn s",
    ),
    (
        "map iteration",
        "local m = { x = 1, y = 2 }\nlocal s = \"\"\nfor k, v in pairs(m) do s = s .. k .. v end\nreturn s",
        "const m = { x: 1, y: 2 }\nlet s = \"\"\nfor (const [key, value] of m) { s += key + value }\nreturn s",
    ),
    (
        "repeat",
        "local n = 0\nrepeat n = n + 1 until n >= 3\nreturn n",
        "let n = 0\ndo { n += 1 } while (n < 3)\nreturn n",
    ),
    (
        "logic",
        "local a, b, c = true, false, false\nreturn a and b or not c",
        "let a = true, b = false, c = false\nreturn a && b || !c",
    ),
    (
        "not equal",
        "return 1 ~= 2",
        "return 1 != 2 && 1 !== 2",
    ),
    (
        "length",
        "local list = {1, 2, 3}\nreturn #list",
        "let list = [1, 2, 3]\nreturn list.length",
    ),
    (
        "string concat",
        "return \"a\" .. \"b\" .. 1",
        "return \"a\" + \"b\" + 1",
    ),
    (
        "comments",
        "-- comment\n--[[ block\ncomment ]]\nreturn 5 -- tail",
        "// comment\n/* block\ncomment */\nreturn 5 // tail",
    ),
    (
        "ternary",
        "local x = 5\nif x > 3 then return \"big\" else return \"small\" end",
        "let x = 5\nreturn x > 3 ? \"big\" : \"small\"",
    ),
    (
        "method colon",
        "local acc = { total = 0 }\nfunction acc:add(n) self.total = self.total + n end\nacc:add(2)\nacc:add(3)\nreturn acc.total",
        "let acc = { total: 0, add: (n) => { acc.total += n } }\nacc.add(2)\nacc.add(3)\nreturn acc.total",
    ),
    (
        "do block",
        "local x = 1\ndo local x = 2 end\nreturn x",
        "let x = 1\n{ let x = 2 }\nreturn x",
    ),
];

#[test]
fn both_spellings_give_the_same_result() {
    for (name, lua, js) in PAIRS {
        let a = run(lua);
        let b = run(js);
        assert_eq!(a, b, "{name}: Lua form gave {a:?}, JS form gave {b:?}");
        assert!(!a.is_nil(), "{name}: result should not be nil");
    }
}

#[test]
fn the_two_spellings_mix_in_one_file() {
    let src = r#"
        local total = 0
        function add(n) {
            total += n
        }
        for i = 1, 3 do add(i) end
        for (const x of [10, 20]) add(x)
        if total > 30 then
            return `total ${total}`
        end
        return "small"
    "#;
    assert_eq!(text(src), "total 36");
}

#[test]
fn operator_precedence() {
    let cases: &[(&str, f64)] = &[
        ("1 + 2 * 3", 7.0),
        ("(1 + 2) * 3", 9.0),
        ("2 ^ 3 ^ 2", 512.0),
        ("2 ** 10", 1024.0),
        ("-2 ^ 2", -4.0),
        ("10 - 4 - 3", 3.0),
        ("100 / 10 / 5", 2.0),
        ("7 % 3", 1.0),
        ("-1 % 3", 2.0),
        ("1 + 2 < 4 and 1 or 0", 1.0),
        ("#[1, 2, 3] * 2", 6.0),
        ("true ? 1 : 2 + 10", 1.0),
        ("false ? 1 : true ? 2 : 3", 2.0),
        ("nil ?? 4", 4.0),
        ("0 ?? 4", 0.0),
        ("not nil and 5 or 6", 5.0),
        ("3 > 2 == true and 1 or 0", 1.0),
    ];
    for (src, want) in cases {
        let got = run(&format!("return {src}"));
        assert_eq!(got, Value::Num(*want), "`{src}`");
    }
    assert_eq!(text("return \"n=\" .. 1 + 2"), "n=3");
    assert_eq!(text("return 1 .. 2"), "12");
}

#[test]
fn numbers_print_without_trailing_zero() {
    assert_eq!(text("return tostring(3)"), "3");
    assert_eq!(text("return tostring(2.5)"), "2.5");
    assert_eq!(text("return tostring(10 / 4)"), "2.5");
    assert_eq!(text("return `${1e3} ${0.1 + 0.2 > 0.3}`"), "1000 true");
    assert_eq!(text("return tostring(-0)"), "0");
}

#[test]
fn truthiness_follows_javascript() {
    assert_eq!(text("return [!!0, !!\"\", !!nil, !!false, !![], !!\"0\", !!1].join()"), "false,false,false,false,true,true,true");
}

#[test]
fn closures_capture_variables() {
    let src = r#"
        function counter() {
            let n = 0
            return () => { n += 1; return n }
        }
        const a = counter()
        const b = counter()
        a(); a()
        b()
        return [a(), b()]
    "#;
    assert_eq!(text(src), "[3, 2]");
    // Переменная цикла — своя на каждой итерации.
    let src = r#"
        let fs = []
        for (const i of [1, 2, 3]) { fs.push(() => i * 10) }
        for i = 4, 5 do fs.push(function() return i end) end
        return fs.map((f) => f()).join(" ")
    "#;
    assert_eq!(text(src), "10 20 30 4 5");
    // Замыкание меняет внешнюю переменную через несколько уровней.
    let src = r#"
        function outer() {
            let x = 1
            function mid() {
                function inner() { x *= 10 }
                inner()
            }
            mid(); mid()
            return x
        }
        return outer()
    "#;
    assert_eq!(run(src), Value::Num(100.0));
}

#[test]
fn functions_are_hoisted_and_recursive() {
    let src = r#"
        return fib(15)
        function fib(n) {
            if (n < 2) return n
            return fib(n - 1) + fib(n - 2)
        }
    "#;
    assert_eq!(run(src), Value::Num(610.0));
    let src = r#"
        function f() {
            return even(10)
            function even(n) { return n == 0 ? true : odd(n - 1) }
            function odd(n) { return n == 0 ? false : even(n - 1) }
        }
        return f()
    "#;
    assert_eq!(run(src), Value::Bool(true));
}

#[test]
fn lists_are_zero_based() {
    assert_eq!(run("let l = [10, 20, 30]\nreturn l[0]"), Value::Num(10.0));
    assert_eq!(run("let l = {10, 20, 30}\nreturn l[2]"), Value::Num(30.0));
    assert_eq!(run("let l = [10]\nreturn l[5]"), Value::Nil);
    assert_eq!(text("let s = ''\nfor i, v in ipairs([7, 8]) do s = s .. i .. ':' .. v .. ' ' end\nreturn s"), "0:7 1:8 ");
    assert_eq!(text("let l = []\nl[0] = 'a'\nl[1] = 'b'\nreturn l"), "[\"a\", \"b\"]");
}

#[test]
fn maps_keep_insertion_order_and_nil_removes() {
    let src = r#"
        let m = { name = "Анна", age: 30, ["key"] = 1, "quoted": 2 }
        m.z = 5
        m.a = 6
        m.age = nil
        return m.keys().join(",")
    "#;
    assert_eq!(text(src), "name,key,quoted,z,a");
    assert_eq!(text("let m = {}\nm[1] = 'one'\nm[true] = 'yes'\nreturn m[1] .. m[true]"), "oneyes");
    assert_eq!(run("let m = {a = 1}\nreturn m.missing"), Value::Nil);
    assert_eq!(text("return {a: {b: [1, {c: 'x'}]}}"), "{a: {b: [1, {c: \"x\"}]}}");
}

#[test]
fn strings_are_indexed_by_characters() {
    assert_eq!(text("let s = 'Привет'\nreturn s[1] .. s.length .. #s"), "р66");
    assert_eq!(text("return 'Привет'.sub(1, 3)"), "ри");
    assert_eq!(text("return 'Привет'.slice(-2)"), "ет");
    assert_eq!(run("return 'abcabc'.find('c')"), Value::Num(2.0));
    assert_eq!(run("return 'abcabc'.find('c', 3)"), Value::Num(5.0));
    assert_eq!(run("return 'abc'.find('z')"), Value::Nil);
    assert_eq!(run("return 'abc'.indexOf('z')"), Value::Num(-1.0));
    assert_eq!(text("return 'a,b,,c'.split(',')"), "[\"a\", \"b\", \"\", \"c\"]");
    assert_eq!(text("return ['x', 'y'].join(' + ')"), "x + y");
    assert_eq!(text("return '  hi  '.trim().upper() .. string.lower('AB')"), "HIab");
    assert_eq!(text("return 'aXbX'.replace('X', '-')"), "a-b-");
    assert_eq!(run("return 'hello'.startsWith('he') and 'hello'.ends_with('lo') and 'hello'.includes('ll')"), Value::Bool(true));
    assert_eq!(text("return string.format('%d/%s %.2f %5d|%-3s|%03d %x %%', 3.7, 'x', 3.14159, 42, 'a', 7, 255)"), "3/x 3.14    42|a  |007 ff %");
    assert_eq!(text("return 'ab'.rep(3)"), "ababab");
    assert_eq!(text("return '7'.padStart(3, '0')"), "007");
    assert_eq!(text("return (3.14159).toFixed(2)"), "3.14");
}

#[test]
fn template_strings() {
    assert_eq!(text("let score = 42\nreturn `Счёт: ${score}`"), "Счёт: 42");
    assert_eq!(text("let a = {b: 2}\nreturn `x${a.b * 2}y${\"}\"}z`"), "x4y}z");
    assert_eq!(text("return `multi\nline`"), "multi\nline");
    assert_eq!(text("return `${[1, 2]} ${nil} ${true}`"), "[1, 2] nil true");
}

#[test]
fn list_methods() {
    let cases: &[(&str, &str)] = &[
        ("let l = [3, 1, 2]\nl.sort()\nreturn l", "[1, 2, 3]"),
        ("let l = [3, 1, 2]\nl.sort((a, b) => a > b)\nreturn l", "[3, 2, 1]"),
        ("let l = [3, 1, 2]\nl.sort((a, b) => a - b)\nreturn l", "[1, 2, 3]"),
        ("let l = ['b', 'a']\nreturn l.sort()", "[\"a\", \"b\"]"),
        ("return [1, 2, 3].map((x) => x * x)", "[1, 4, 9]"),
        ("return [1, 2, 3, 4].filter((x) => x % 2 == 0)", "[2, 4]"),
        ("return [1, 2, 3].find((x) => x > 1)", "2"),
        ("return [1, 2, 3].find_index((x) => x > 1)", "1"),
        ("return [1, 2, 3].reduce((a, b) => a + b)", "6"),
        ("return [1, 2, 3].reduce((a, b) => a + b, 10)", "16"),
        ("return [1, 2, 3].includes(2) and [1].contains(1)", "true"),
        ("let l = [1, 2]\nl.push(3, 4)\nreturn l.pop() + l.length", "7"),
        ("let l = [1, 2, 3]\nl.insert(1, 9)\nreturn l", "[1, 9, 2, 3]"),
        ("let l = [1, 2, 3]\nlet r = l.remove(0)\nreturn [r, l]", "[1, [2, 3]]"),
        ("return [1, 2, 3].reverse()", "[3, 2, 1]"),
        ("return [1, 2, 3, 4].slice(1, 3)", "[2, 3]"),
        ("return [1, 2].concat([3], [4])", "[1, 2, 3, 4]"),
        ("return [5, 6].indexOf(6)", "1"),
        ("let n = 0\n[1, 2, 3].each((x) => { n += x })\nreturn n", "6"),
        ("return [1, 2, 3].some((x) => x > 2) and [1, 2].every((x) => x > 0)", "true"),
        ("let t = [1, 2]\ntable.insert(t, 3)\nreturn table.concat(t, '-')", "1-2-3"),
        ("return [[1, 2], [3]].map((p) => #p)", "[2, 1]"),
        ("let words = ['pear', 'fig', 'apple']\nwords.sort((a, b) => #a < #b)\nreturn words", "[\"fig\", \"pear\", \"apple\"]"),
        ("return [1, 2, 3].sum()", "6"),
        ("return range(4)", "[0, 1, 2, 3]"),
        ("return range(1, 10, 3)", "[1, 4, 7]"),
    ];
    for (src, want) in cases {
        assert_eq!(text(src), *want, "`{src}`");
    }
}

#[test]
fn map_methods() {
    let cases: &[(&str, &str)] = &[
        ("let m = {a: 1, b: 2}\nreturn m.values()", "[1, 2]"),
        ("let m = {a: 1}\nreturn [m.has('a'), m.has('b')]", "[true, false]"),
        ("let m = {a: 1}\nreturn m.get('b', 5)", "5"),
        ("let m = {a: 1, b: 2}\nm.remove('a')\nreturn m", "{b: 2}"),
        ("let m = {a: 1, b: 2}\nreturn m.size() + #m", "4"),
        ("let m = {a: 1}\nlet s = ''\nm.each((k, v) => { s = k .. v })\nreturn s", "a1"),
        ("return keys({x: 1, y: 2})", "[\"x\", \"y\"]"),
        ("let m = {count: 1}\nm.count += 2\nm['count'] *= 2\nreturn m.count", "6"),
    ];
    for (src, want) in cases {
        assert_eq!(text(src), *want, "`{src}`");
    }
}

#[test]
fn math_and_random_are_deterministic() {
    assert_eq!(text("return [math.floor(2.7), math.ceil(2.1), math.round(2.5), math.abs(-3), math.max(1, 5, 3), math.min([4, 2])]"), "[2, 3, 3, 3, 5, 2]");
    assert_eq!(text("return [math.clamp(5, 0, 3), math.lerp(0, 10, 0.25), math.sqrt(16), math.pow(2, 8)]"), "[3, 2.5, 4, 256]");
    assert_eq!(run("return math.pi > 3.14 and math.pi < 3.15"), Value::Bool(true));
    let a = run("seed(7)\nreturn [random(1, 6), random(1, 6), random(), choose(['a', 'b', 'c'])]");
    let b = run("seed(7)\nreturn [random(1, 6), random(1, 6), random(), choose(['a', 'b', 'c'])]");
    assert_eq!(a.to_text(), b.to_text());
    let v = run("let ok = true\nfor i = 1, 200 do let r = random(1, 3)\nif r < 1 or r > 3 or r != math.floor(r) then ok = false end end\nreturn ok");
    assert_eq!(v, Value::Bool(true));
}

#[test]
fn vectors_and_colors_are_values() {
    assert_eq!(text("let v = vec3(1, 2, 3)\nlet w = v\nw.x = 10\nreturn [v.x, w.x]"), "[1, 10]");
    assert_eq!(text("let p = {pos: vec2(1, 2)}\np.pos.y += 5\nreturn p.pos"), "vec2(1, 7)");
    assert_eq!(text("return vec2(1, 2) + vec2(3, 4) * 2"), "vec2(7, 10)");
    assert_eq!(run("return vec3(3, 4, 0).length()"), Value::Num(5.0));
    assert_eq!(text("return color('#ff0000').lerp(color(0, 0, 1), 0.5)"), "color(0.5, 0, 0.5, 1)");
    assert_eq!(text("let c = color(1, 1, 1)\nc.a = 0.5\nreturn c"), "color(1, 1, 1, 0.5)");
    assert_eq!(run("return vec2(1, 2) == vec2(1, 2)"), Value::Bool(true));
    assert_eq!(run("return vec2(0.1, 0).x"), Value::Num(0.1));
}

#[test]
fn rest_parameters_and_spread() {
    assert_eq!(text("function f(a, ...rest) { return [a, rest] }\nreturn f(1, 2, 3)"), "[1, [2, 3]]");
    assert_eq!(text("function f(a, b, c) { return a + b + c }\nlet xs = [1, 2, 3]\nreturn f(...xs)"), "6");
    assert_eq!(text("let a = [1, 2]\nreturn [0, ...a, 3]"), "[0, 1, 2, 3]");
    assert_eq!(text("function f(a, b) { return [a, b] }\nreturn f(1)"), "[1, nil]");
    assert_eq!(text("function f(a) { return a }\nreturn f(1, 2, 3)"), "1");
}

#[test]
fn multiple_assignment_and_destructuring() {
    assert_eq!(text("local a, b = 1, 2\na, b = b, a\nreturn [a, b]"), "[2, 1]");
    assert_eq!(text("let [x, y] = [5, 6]\nreturn x * y"), "30");
    assert_eq!(text("let ok, res = pcall(() => 7)\nreturn [ok, res]"), "[true, 7]");
    assert_eq!(text("let ok, err = pcall(() => error('boom'))\nreturn [ok, err]"), "[false, \"boom\"]");
}

#[test]
fn try_catch_and_throw() {
    assert_eq!(text("try { error('bad') } catch (e) { return 'caught ' + e }"), "caught bad");
    assert_eq!(text("try { let x = nil\nx.y = 1 } catch (e) { return e }"), "tried to set `y` on nil — `x` is nil");
    assert_eq!(text("try\n  throw {code: 5}\ncatch e\n  return e.code\nend"), "5");
    // try внутри цикла с break и вызовами.
    let src = r#"
        let log = []
        function risky(i) { if (i == 2) error("two") return i }
        for (let i = 0; i < 5; i++) {
            try {
                log.push(risky(i))
                if (i == 3) break
            } catch (e) { log.push(e) }
        }
        try { error("after") } catch (e) { log.push(e) }
        return log
    "#;
    assert_eq!(text(src), "[0, 1, \"two\", 3, \"after\"]");
}

#[test]
fn break_and_continue() {
    assert_eq!(text("let s = []\nfor (let i = 0; i < 10; i++) { if (i % 2 == 1) continue\nif (i > 6) break\ns.push(i) }\nreturn s"), "[0, 2, 4, 6]");
    assert_eq!(text("let s = []\nfor i = 1, 10 do if i == 3 then continue end if i == 5 then break end s.push(i) end\nreturn s"), "[1, 2, 4]");
    assert_eq!(text("let n = 0\nwhile true do n += 1\nif n == 4 then break end end\nreturn n"), "4");
    assert_eq!(text("let n = 0\nrepeat n += 1\nif n < 3 then continue end until n >= 5\nreturn n"), "5");
}

#[test]
fn numeric_for_with_step() {
    assert_eq!(text("let s = []\nfor i = 10, 1, -3 do s.push(i) end\nreturn s"), "[10, 7, 4, 1]");
    assert_eq!(text("let s = []\nfor i = 0, 1, 0.25 do s.push(i) end\nreturn s"), "[0, 0.25, 0.5, 0.75, 1]");
    assert_eq!(text("let s = []\nfor i = 5, 1 do s.push(i) end\nreturn s"), "[]");
}

#[test]
fn for_in_over_maps_lists_and_strings() {
    assert_eq!(text("let s = ''\nfor (const k in {a: 1, b: 2}) s += k\nreturn s"), "ab");
    assert_eq!(text("let s = ''\nfor (const i in [5, 6]) s += i\nreturn s"), "01");
    assert_eq!(text("let s = ''\nfor ch of 'дом' do s = ch .. s end\nreturn s"), "мод");
    assert_eq!(text("let s = ''\nfor k, v in {x = 1} do s = k .. '=' .. v end\nreturn s"), "x=1");
    // Изменение списка во время обхода видно циклу.
    assert_eq!(text("let l = [1]\nlet n = 0\nfor (const x of l) { n++\nif (#l < 3) l.push(x + 1) }\nreturn n"), "3");
}

#[test]
fn newline_does_not_glue_a_call_onto_the_previous_line() {
    // Без правила `(` склеилось бы в вызов `b(...)`.
    let src = "let a = 1\nlet b = 2\nlet c = b\n(a)\nreturn c";
    let mut vm = Vm::new();
    // `(a)` — отдельная инструкция, и она ничего не делает: это ошибка, а не вызов `b(a)`.
    let e = vm.run_source("t.scenario", src).unwrap_err();
    assert_eq!(e.pos.line, 4, "{e}");
    // А оператор в конце строки продолжает выражение.
    assert_eq!(run("let x = 1 +\n  2\nreturn x"), Value::Num(3.0));
    assert_eq!(run("let x = [1, 2]\n  .map((v) => v * 2)\n  .sum()\nreturn x"), Value::Num(6.0));
    assert_eq!(run("let t = true\n  && false\nreturn t"), Value::Bool(false));
}

#[test]
fn increments_and_compound_assignment() {
    assert_eq!(text("let i = 1\ni++\ni += 2\ni *= 3\ni -= 1\ni /= 2\ni %= 4\nreturn i"), "1.5");
    assert_eq!(text("let s = 'a'\ns ..= 'b'\ns += 'c'\nreturn s"), "abc");
    assert_eq!(text("let l = [1, 2]\nl[1] += 5\nl[0]++\nreturn l"), "[2, 7]");
}

#[test]
fn typeof_and_conversions() {
    assert_eq!(text("return [typeof 1, typeof 'a', typeof nil, typeof [], typeof {}, typeof print, type(true), typeof(vec2())]"),
        "[\"number\", \"string\", \"nil\", \"list\", \"map\", \"function\", \"boolean\", \"vec2\"]");
    assert_eq!(text("return [tonumber('12'), tonumber(' 3.5 '), tonumber('x'), tonumber('ff', 16), tonumber('0x10'), tostring(nil)]"),
        "[12, 3.5, nil, 255, 16, \"nil\"]");
    assert_eq!(run("return null == nil and 1 === 1"), Value::Bool(true));
}

#[test]
fn field_functions_and_lua_modules() {
    let src = r#"
        local M = {}
        function M.square(x) return x * x end
        function M:twice(x) return self.square(x) * 2 end
        return M:twice(3)
    "#;
    assert_eq!(run(src), Value::Num(18.0));
}

#[test]
fn print_goes_to_the_output() {
    let mut vm = Vm::new();
    vm.run_source("t.scenario", "print('a', 1, [2])\nlog('b')\nwarn('c')").unwrap();
    assert_eq!(vm.take_output(), vec!["a 1 [2]".to_string(), "b".into(), "warning: c".into()]);
}

#[test]
fn value_is_small() {
    // Строка — толстый указатель `Rc<str>` (16 байт) + тег.
    assert!(std::mem::size_of::<Value>() <= 24, "Value is {} bytes", std::mem::size_of::<Value>());
}

/// Фрагменты из docs/api/scenario.md, которым не нужен движок.
#[test]
fn documentation_snippets_work() {
    let cases: &[(&str, &str)] = &[
        ("return [3, 1, 2].sort().join()", "1,2,3"),
        ("let r\ntry error('x') catch e r = e end\nreturn r", "x"),
        ("local ok, result = pcall(function(a, b) return a + b end, 1, 2)\nreturn [ok, result]", "[true, 3]"),
        ("let e\ntry { throw { code: 5 } } catch (x) { e = x.code }\nreturn e", "5"),
        ("let s = ''\nfor ch of 'дом' do s ..= ch end\nreturn s", "дом"),
        ("do local tmp = 1 end\nreturn 1", "1"),
        ("return format('%05.1f', 3.14159)", "003.1"),
        ("return [math.round(2.5), math.round(-2.5), -1 % 3, 2 ^ 3 ^ 2, -2 ^ 2]", "[3, -2, 2, 512, -4]"),
        ("return 1 == '1'", "false"),
        ("return 'Привет'[1]", "р"),
        ("let v = vec2(1, 2)\nlet w = v\nw.x = 5\nreturn v.x", "1"),
        ("const inc = x => x + 1\nconst twice = (x) => x * 2\nreturn twice(inc(2))", "6"),
        ("function sum(first, ...rest) { return first + rest.sum() }\nreturn sum(...[1, 2, 3])", "6"),
        ("let stats = { killed = 0, wave: 1 }\nlet s = ''\nfor (const [key, value] of stats) s += key + value\nreturn s", "killed0wave1"),
        ("let total = 2 +\n    3\nreturn total", "5"),
        ("let m = {a = 1}\nm.a = nil\nreturn m.has('a')", "false"),
        ("return ['a', 'b'].map((x, i) => x + i)", "[\"a0\", \"b1\"]"),
        ("return [range(3), range(1, 4)]", "[[0, 1, 2], [1, 2, 3]]"),
        ("return [tonumber('x'), tostring(1.50)]", "[nil, \"1.5\"]"),
    ];
    for (src, want) in cases {
        assert_eq!(text(src), *want, "`{src}`");
    }
}

/// Поля значений, которые даёт движок (`self`, `scene`…), можно менять: имя не объявлено
/// в скрипте, но присваивание идёт в поле, а не в саму переменную.
#[test]
fn fields_of_host_values_can_be_assigned() {
    let mut vm = Vm::new();
    let host = vm.run_source("host.scenario", "return {rotation: 0, pos: vec2(1, 2)}").unwrap();
    let origin = vm.run_source("host.scenario", "return vec2(1, 2)").unwrap();
    vm.register_value("me", host);
    vm.register_value("origin", origin);
    let src = "me.rotation = 90\nme.rotation += 1\nme.pos.x = 5\nme[\"tag\"] = \"a\"\norigin.x = 7\n\
               function turn() me.rotation = me.rotation + 1 end\nturn()\n\
               return [me.rotation, me.pos.x, me.tag, origin.x]";
    let v = vm.run_source("test.scenario", src).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(v.to_text(), "[92, 5, \"a\", 7]");
    // Сама переменная движка по-прежнему не переназначается.
    let err = vm.run_source("test.scenario", "me = 1").unwrap_err().to_string();
    assert!(err.contains("is not declared"), "{err}");
}

//! Небольшой замер скорости интерпретатора:
//! `cargo run -p engine-scenario --example bench` (профиль dev).

use std::time::Instant;

use engine_scenario::{Script, Value, Vm};

const CASES: &[(&str, &str)] = &[
    (
        "1 000 000 loop iterations with arithmetic (locals)",
        "function run() {\n  let s = 0\n  for (let i = 0; i < 1000000; i++) { s = (s + i * 3) % 1000003 }\n  return s\n}\nreturn run()",
    ),
    (
        "1 000 000 loop iterations (top-level variables, Lua form)",
        "local s = 0\nfor i = 0, 999999 do s = (s + i * 3) % 1000003 end\nreturn s",
    ),
    ("recursive fib(25)", "function fib(n) { if (n < 2) return n\n return fib(n - 1) + fib(n - 2) }\nreturn fib(25)"),
    (
        "push 100 000 numbers + sort",
        "seed(1)\nlet l = []\nfor (let i = 0; i < 100000; i++) { l.push(random()) }\nl.sort()\nreturn l.length",
    ),
    (
        "sort 100 000 numbers with a comparator",
        "seed(1)\nlet l = []\nfor (let i = 0; i < 100000; i++) { l.push(random()) }\nl.sort((a, b) => a < b)\nreturn l.length",
    ),
    (
        "100 000 map writes + reads",
        "let m = {}\nfor (let i = 0; i < 100000; i++) { m['k' + (i % 1000)] = i }\nlet s = 0\nfor (const [k, v] of m) { s += v }\nreturn s",
    ),
];

fn main() {
    let mut vm = Vm::new();
    vm.limits.instructions_per_resume = u32::MAX;
    for (name, src) in CASES {
        let script = Script::compile("bench.scenario", src).expect("compiles");
        let start = Instant::now();
        let v = vm.run_script(&script, &mut (), 1.0 / 60.0, 10).expect("runs");
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let shown = match &v {
            Value::Num(n) => engine_scenario::fmt_num(*n),
            other => other.to_text(),
        };
        println!("{ms:9.1} ms  {name}  (= {shown})");
    }
}

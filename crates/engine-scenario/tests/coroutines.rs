//! Сценарии — сопрограммы: `wait` приостанавливает задачу посреди вложенных вызовов и
//! циклов, задачи работают параллельно, сообщения будят ожидающих и запускают обработчики.

use std::rc::Rc;

use engine_scenario::{Group, Script, Suspend, Value, Vm};

fn group(vm: &Vm, src: &str) -> Group {
    let script = Script::compile("co.scenario", src).unwrap_or_else(|e| panic!("{e}"));
    let mut g = Group::new(0);
    g.start(vm, &script).unwrap_or_else(|e| panic!("{e}"));
    g
}

fn step(vm: &mut Vm, g: &mut Group, dt: f64) {
    let r = g.update(vm, &mut (), dt, &mut |_, _| None);
    assert!(r.errors.is_empty(), "{}", r.errors[0]);
}

fn out(vm: &mut Vm) -> Vec<String> {
    vm.take_output()
}

#[test]
fn wait_suspends_inside_nested_calls_and_loops() {
    let mut vm = Vm::new();
    let src = r#"
        function inner(i) {
            print("inner " + i)
            wait(0.5)
            return i * 10
        }
        function outer() {
            let total = 0
            for (let i = 1; i <= 2; i++) {
                total += inner(i)
            }
            return total
        }
        print("result " + outer())
    "#;
    let mut g = group(&vm, src);
    step(&mut vm, &mut g, 0.25);
    assert_eq!(out(&mut vm), ["inner 1"]);
    step(&mut vm, &mut g, 0.25);
    assert_eq!(out(&mut vm), Vec::<String>::new());
    step(&mut vm, &mut g, 0.25); // прошло 0.5 с — второй вызов
    assert_eq!(out(&mut vm), ["inner 2"]);
    step(&mut vm, &mut g, 0.25);
    step(&mut vm, &mut g, 0.25);
    assert_eq!(out(&mut vm), ["result 30"]);
    assert!(g.is_idle());
}

#[test]
fn wait_timing_with_fixed_dt() {
    let mut vm = Vm::new();
    let mut g = group(&vm, "let t = 0\nwhile (t < 3) { wait(0.3)\nt += 1\nprint(`tick ${t} at ${math.round(time() * 10)}`) }");
    let mut ticks = Vec::new();
    for frame in 1..=12 {
        step(&mut vm, &mut g, 0.1);
        for line in out(&mut vm) {
            ticks.push((frame, line));
        }
    }
    // Главная задача стартует в кадре 1, wait(0.3) кончается через три кадра.
    assert_eq!(
        ticks,
        vec![(4, "tick 1 at 4".to_string()), (7, "tick 2 at 7".to_string()), (10, "tick 3 at 10".to_string())]
    );
}

#[test]
fn parallel_tasks_and_stop() {
    let mut vm = Vm::new();
    let src = r#"
        let log = []
        const blinker = start(() => {
            while (true) { log.push("b"); wait_frame() }
        })
        start((name) => { wait_frame(2); log.push(name) }, "once")
        wait_frame(3)
        stop(blinker)
        wait_frame(2)
        print(log.join(""))
        print(blinker.running, blinker.done)
    "#;
    let mut g = group(&vm, src);
    for _ in 0..8 {
        step(&mut vm, &mut g, 1.0 / 60.0);
    }
    // b в кадрах 1, 2, 3; «once» — в кадре 3 (после b этого кадра); главная задача идёт
    // первой и останавливает мигалку в кадре 4 до её хода.
    assert_eq!(out(&mut vm), ["bbbonce", "false true"]);
}

#[test]
fn messages_wake_waiters_and_run_handlers() {
    let mut vm = Vm::new();
    let src = r#"
        let hits = 0
        on("hit", (dmg) => {
            hits += dmg
            wait(1)          -- обработчик — своя задача, он может ждать
            print("handled " + dmg)
        })
        let v = wait_message("open")
        print("opened with " + v + ", hits " + hits)
    "#;
    let mut g = group(&vm, src);
    step(&mut vm, &mut g, 0.5);
    g.post("hit", Value::Num(3.0));
    g.post("hit", Value::Num(4.0));
    step(&mut vm, &mut g, 0.5);
    assert!(out(&mut vm).is_empty());
    g.post("open", Value::str("key"));
    step(&mut vm, &mut g, 0.5);
    assert_eq!(out(&mut vm), ["opened with key, hits 7"]);
    step(&mut vm, &mut g, 0.5);
    assert_eq!(out(&mut vm), ["handled 3", "handled 4"]);
    assert!(g.listens("hit"));
}

#[test]
fn emit_reaches_own_handlers_next_frame() {
    let mut vm = Vm::new();
    let src = r#"
        on("ping", (v) => print("got " + v))
        emit("ping", 1)
        print("sent")
    "#;
    let mut g = group(&vm, src);
    step(&mut vm, &mut g, 0.1);
    assert_eq!(out(&mut vm), ["sent"]);
    step(&mut vm, &mut g, 0.1);
    assert_eq!(out(&mut vm), ["got 1"]);
}

#[test]
fn wait_until_and_wait_task() {
    let mut vm = Vm::new();
    let src = r#"
        let door = false
        const opener = start(() => { wait(0.2); door = true })
        wait_until(() => door)
        print("door at " + math.round(time() * 10))
        const t = start(() => wait(0.3))
        wait_task(t)
        print("task done at " + math.round(time() * 10))
        print(wait_until(() => false, 0.2))
    "#;
    let mut g = group(&vm, src);
    for _ in 0..12 {
        step(&mut vm, &mut g, 0.1);
    }
    // Задачи в кадре идут по порядку запуска: главная видит открытую дверь в кадре после
    // того, как её открыли (кадр 4), и конец задачи — в кадре после её завершения (8).
    assert_eq!(out(&mut vm), ["door at 4", "task done at 8", "false"]);
}

#[test]
fn callbacks_inside_list_methods_may_wait() {
    let mut vm = Vm::new();
    let src = r#"
        const r = [1, 2, 3].map((x) => { wait_frame(); return x * 2 })
        print(r.join(","))
    "#;
    let mut g = group(&vm, src);
    for _ in 0..3 {
        step(&mut vm, &mut g, 0.1);
        assert!(out(&mut vm).is_empty());
    }
    step(&mut vm, &mut g, 0.1);
    assert_eq!(out(&mut vm), ["2,4,6"]);
}

#[test]
fn host_waits_are_polled() {
    let mut vm = Vm::new();
    vm.register("wait_door", |ctx, _| {
        ctx.suspend(Suspend::Host(Value::str("door")));
        Ok(Value::Nil)
    });
    let mut g = group(&vm, "let r = wait_door()\nprint('door says ' + r)");
    let mut open = false;
    for frame in 0..5 {
        if frame == 3 {
            open = true;
        }
        let r = g.update(&mut vm, &mut (), 0.1, &mut |_, token| {
            assert_eq!(token, &Value::str("door"));
            open.then(|| Value::str("open"))
        });
        assert!(r.errors.is_empty());
    }
    assert_eq!(out(&mut vm), ["door says open"]);
}

#[test]
fn natives_reach_the_host() {
    struct Game {
        coins: i64,
    }
    let mut vm = Vm::new();
    vm.register("give_coins", |ctx, args| {
        let n = args.int(0)?;
        let game = ctx.host::<Game>().ok_or("no game")?;
        game.coins += n;
        Ok(Value::Num(game.coins as f64))
    });
    let script = Script::compile("host.scenario", "give_coins(5)\nreturn give_coins(2)").unwrap();
    let mut game = Game { coins: 1 };
    let v = vm.run_script(&script, &mut game, 0.1, 10).unwrap();
    assert_eq!(v, Value::Num(8.0));
    assert_eq!(game.coins, 8);
}

#[test]
fn host_classes_have_properties_and_methods() {
    let mut vm = Vm::new();
    let class = vm.register_class("door");
    vm.class_getter(class, |_, id, name| Ok(match name {
        "id" => Some(Value::Num(id as f64)),
        "pos" => Some(Value::Vec2(id as f32, 0.0)),
        _ => None,
    }));
    let state: Rc<std::cell::RefCell<Vec<String>>> = Rc::default();
    let s2 = state.clone();
    vm.class_setter(class, move |_, id, name, v| {
        s2.borrow_mut().push(format!("{id}.{name}={}", v.to_text()));
        Ok(name == "pos")
    });
    vm.class_method(class, "open", |_, a| Ok(Value::str(&format!("opened {}", a.handle(0, 1, "a door")?))));
    vm.register_value("door7", Value::Handle(engine_scenario::Handle { class, id: 7 }));
    let v = vm.run_source("c.scenario", "door7.pos.x = 3\nreturn [door7.id, door7:open(), door7.open(), typeof door7]").unwrap();
    assert_eq!(v.to_text(), "[7, \"opened 7\", \"opened 7\", \"object\"]");
    assert_eq!(state.borrow().as_slice(), ["7.pos=vec2(3, 0)"]);
    let e = vm.run_source("c.scenario", "door7.opne()").unwrap_err();
    assert!(e.message.contains("has no method `opne`"), "{e}");
    assert_eq!(e.hint.as_deref(), Some("did you mean `open`?"));
}

#[test]
fn stop_all_from_a_script_keeps_the_caller() {
    let mut vm = Vm::new();
    let src = r#"
        start(() => { while (true) wait_frame() })
        start(() => { while (true) wait_frame() })
        wait_frame()
        stop_all()
        wait_frame()
        print("still here")
    "#;
    let mut g = group(&vm, src);
    step(&mut vm, &mut g, 0.1);
    assert_eq!(g.task_count(), 3);
    step(&mut vm, &mut g, 0.1);
    assert_eq!(g.task_count(), 1);
    step(&mut vm, &mut g, 0.1);
    assert_eq!(out(&mut vm), ["still here"]);
    assert!(g.is_idle());
}

#[test]
fn same_script_and_seed_give_the_same_run() {
    let src = r#"
        seed(42)
        let xs = []
        for (let i = 0; i < 20; i++) { xs.push(random(1, 100)); if (i % 5 == 0) wait_frame() }
        let l = range(10)
        l.shuffle()
        print(xs.join(","), l.join(","))
    "#;
    let run = || {
        let mut vm = Vm::new();
        let mut g = group(&vm, src);
        for _ in 0..10 {
            step(&mut vm, &mut g, 0.1);
        }
        vm.take_output()
    };
    let a = run();
    assert_eq!(a.len(), 1);
    assert_eq!(a, run());
}

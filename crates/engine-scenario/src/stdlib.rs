//! Стандартная библиотека: глобальные функции, `math`, `string`, `table`, методы
//! строк, списков, словарей, векторов и цветов, сопрограммы (`wait`, `start`, `on`).

use std::rc::Rc;

use crate::compiler::compile_script;
use crate::task::{char_len, check_str, Task};
use crate::value::*;
use crate::vm::{native, Level, NativeCtx, SchedCtx, Status, Suspend, TypeTag, Vm};

type F = Rc<dyn Fn(&mut NativeCtx, Args) -> NativeResult>;

/// Метод типа и одноимённая функция пространства имён (`s.upper()` и `string.upper(s)`).
fn both(vm: &mut Vm, ns: &mut OrderedMap, tag: TypeTag, names: &[&str], f: F) {
    for name in names {
        let g = f.clone();
        vm.method_value(tag, name, native(name, true, move |c, a| g(c, a)));
        let g = f.clone();
        ns.insert(Key::Str((*name).into()), native(name, false, move |c, a| g(c, a)));
    }
}

fn method(vm: &mut Vm, tag: TypeTag, names: &[&str], f: F) {
    for name in names {
        let g = f.clone();
        vm.method_value(tag, name, native(name, true, move |c, a| g(c, a)));
    }
}

fn func(vm: &mut Vm, names: &[&str], f: F) {
    for name in names {
        let g = f.clone();
        vm.register_value(name, native(name, false, move |c, a| g(c, a)));
    }
}

fn ns_fn(ns: &mut OrderedMap, name: &str, f: F) {
    ns.insert(Key::Str(name.into()), native(name, false, move |c, a| f(c, a)));
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// Индекс в стиле JS slice: отрицательные — с конца, обрезка в [0, len].
fn rel_index(i: f64, len: usize) -> usize {
    let i = i.trunc();
    if i < 0.0 {
        (len as f64 + i).max(0.0) as usize
    } else {
        (i as usize).min(len)
    }
}

fn text_args(args: &Args) -> String {
    args.values.iter().map(Value::to_text).collect::<Vec<_>>().join(" ")
}

fn num_list(args: &Args) -> Result<Vec<f64>, Throw> {
    let vals: Vec<Value> = match (args.len(), args.get(0)) {
        (1, Value::List(l)) => l.to_vec(),
        _ => args.values.to_vec(),
    };
    if vals.is_empty() {
        return Err(Throw::msg(format!("`{}` needs at least one number", args.name)));
    }
    vals.iter()
        .map(|v| v.as_num().ok_or_else(|| Throw::msg(format!("`{}` works on numbers, got {}", args.name, v.describe()))))
        .collect()
}

/// `random()`, `random(n)`, `random(a, b)`.
fn random_value(ctx: &mut NativeCtx, a: Args) -> NativeResult {
    match a.len() {
        0 => Ok(Value::Num(ctx.rng().float())),
        1 => {
            let n = a.int(0)?;
            if n <= 0 {
                return Err(Throw::msg("`random(n)` needs n > 0"));
            }
            Ok(Value::Num(ctx.rng().int(0, n - 1) as f64))
        }
        _ => {
            let (lo, hi) = (a.num(0)?, a.num(1)?);
            if lo.fract() == 0.0 && hi.fract() == 0.0 {
                Ok(Value::Num(ctx.rng().int(lo as i64, hi as i64) as f64))
            } else {
                let t = ctx.rng().float();
                Ok(Value::Num(lo + (hi - lo) * t))
            }
        }
    }
}

/// Подмножество printf: `%d %i %f %.2f %s %x %X %%`, флаги `-` и `0`, ширина.
pub fn format(fmt: &str, args: &[Value]) -> Result<String, Throw> {
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    let mut n = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if it.peek() == Some(&'%') {
            it.next();
            out.push('%');
            continue;
        }
        let mut left = false;
        let mut zero = false;
        while let Some(&f) = it.peek() {
            match f {
                '-' => left = true,
                '0' => zero = true,
                _ => break,
            }
            it.next();
        }
        let mut width = 0usize;
        while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
            width = width * 10 + d as usize;
            it.next();
        }
        let mut prec: Option<usize> = None;
        if it.peek() == Some(&'.') {
            it.next();
            let mut p = 0;
            while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                p = p * 10 + d as usize;
                it.next();
            }
            prec = Some(p);
        }
        let Some(conv) = it.next() else {
            return Err(Throw::msg("format string ends with `%` — write `%%` for a percent sign"));
        };
        let arg = args.get(n).cloned().unwrap_or_default();
        n += 1;
        let num = |what: &str| -> Result<f64, Throw> {
            arg.as_num().ok_or_else(|| Throw::msg(format!("`%{what}` in format needs a number, got {}", arg.describe())))
        };
        let body = match conv {
            'd' | 'i' => format!("{}", num("d")?.trunc() as i64),
            'f' => format!("{:.*}", prec.unwrap_or(6), num("f")?),
            'g' => fmt_num(num("g")?),
            'x' => format!("{:x}", num("x")?.trunc() as i64),
            'X' => format!("{:X}", num("X")?.trunc() as i64),
            's' => {
                let t = arg.to_text();
                match prec {
                    Some(p) => t.chars().take(p).collect(),
                    None => t,
                }
            }
            'q' => format!("{:?}", arg.to_text()),
            other => return Err(Throw::msg(format!("unknown format `%{other}` — use %d, %f, %.2f, %s or %x"))),
        };
        let len = body.chars().count();
        if len < width {
            let pad = width - len;
            if left {
                out.push_str(&body);
                out.extend(std::iter::repeat_n(' ', pad));
            } else if zero && !matches!(conv, 's' | 'q') {
                let (sign, digits) = if let Some(d) = body.strip_prefix('-') { ("-", d) } else { ("", body.as_str()) };
                out.push_str(sign);
                out.extend(std::iter::repeat_n('0', pad));
                out.push_str(digits);
            } else {
                out.extend(std::iter::repeat_n(' ', pad));
                out.push_str(&body);
            }
        } else {
            out.push_str(&body);
        }
    }
    Ok(out)
}

fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    if t.is_empty() || t.contains(|c: char| c.is_alphabetic() && c != 'e' && c != 'E') {
        return None;
    }
    t.parse::<f64>().ok()
}

fn color_from_hex(s: &str) -> Option<Value> {
    let h = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| b as f32 / 255.0);
    match h.len() {
        6 => Some(Value::color(byte(0)?, byte(2)?, byte(4)?, 1.0)),
        8 => Some(Value::color(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

fn vec_of(v: &Value) -> Option<[f32; 3]> {
    match v {
        Value::Vec2(x, y) => Some([*x, *y, 0.0]),
        Value::Vec3(x, y, z) => Some([*x, *y, *z]),
        _ => None,
    }
}

fn same_kind(template: &Value, c: [f32; 3]) -> Value {
    match template {
        Value::Vec2(..) => Value::Vec2(c[0], c[1]),
        _ => Value::Vec3(c[0], c[1], c[2]),
    }
}

pub(crate) fn install(vm: &mut Vm) {
    install_globals(vm);
    install_math(vm);
    install_strings(vm);
    install_lists(vm);
    install_maps(vm);
    install_vectors(vm);
    install_tasks(vm);
    install_prelude(vm);
}

fn install_globals(vm: &mut Vm) {
    func(vm, &["print", "log"], Rc::new(|c, a| {
        let t = text_args(&a);
        c.print(Level::Info, &t);
        Ok(Value::Nil)
    }));
    func(vm, &["warn"], Rc::new(|c, a| {
        let t = text_args(&a);
        c.print(Level::Warn, &t);
        Ok(Value::Nil)
    }));
    func(vm, &["type"], Rc::new(|_, a| Ok(Value::str(a.get(0).type_name()))));
    func(vm, &["tostring"], Rc::new(|_, a| Ok(Value::Str(a.get(0).to_text().into()))));
    func(vm, &["tonumber"], Rc::new(|_, a| {
        Ok(match a.get(0) {
            Value::Num(n) => Value::Num(*n),
            Value::Str(s) => match a.get(1) {
                Value::Num(base) if (2.0..=36.0).contains(base) => {
                    i64::from_str_radix(s.trim(), *base as u32).map(|v| Value::Num(v as f64)).unwrap_or_default()
                }
                _ => parse_number(s).map(Value::Num).unwrap_or_default(),
            },
            Value::Bool(b) => Value::Num(if *b { 1.0 } else { 0.0 }),
            _ => Value::Nil,
        })
    }));
    func(vm, &["assert"], Rc::new(|_, a| {
        if a.get(0).truthy() {
            Ok(a.get(0).clone())
        } else {
            match a.get(1) {
                Value::Nil => Err(Throw::msg("assertion failed")),
                v => Err(Throw::Msg(v.to_text())),
            }
        }
    }));
    func(vm, &["error"], Rc::new(|_, a| Err(Throw::Value(match a.get(0) {
        Value::Nil => Value::str("error"),
        v => v.clone(),
    }))));
    func(vm, &["len"], Rc::new(|_, a| {
        Ok(Value::Num(match a.get(0) {
            Value::List(l) => l.len(),
            Value::Str(s) => char_len(s),
            Value::Map(m) => m.borrow().len(),
            _ => return Err(a.bad(0, "a list, string or map")),
        } as f64))
    }));
    func(vm, &["ipairs"], Rc::new(|_, a| {
        a.list(0)?;
        Ok(a.get(0).clone())
    }));
    func(vm, &["pairs"], Rc::new(|_, a| match a.get(0) {
        v @ (Value::List(_) | Value::Map(_) | Value::Str(_)) => Ok(v.clone()),
        _ => Err(a.bad(0, "a map or a list")),
    }));
    func(vm, &["keys"], Rc::new(|_, a| match a.get(0) {
        Value::Map(m) => Ok(Value::list(m.borrow().iter().map(|(k, _)| k.to_value()).collect())),
        Value::List(l) => Ok(Value::list((0..l.len()).map(|i| Value::Num(i as f64)).collect())),
        _ => Err(a.bad(0, "a map")),
    }));
    func(vm, &["values"], Rc::new(|_, a| match a.get(0) {
        Value::Map(m) => Ok(Value::list(m.borrow().iter().map(|(_, v)| v.clone()).collect())),
        Value::List(l) => Ok(Value::list(l.to_vec())),
        _ => Err(a.bad(0, "a map")),
    }));
    func(vm, &["range"], Rc::new(|c, a| {
        let (start, end) = if a.len() >= 2 { (a.num(0)?, a.num(1)?) } else { (0.0, a.num(0)?) };
        let step = a.opt_num(2, 1.0)?;
        if step == 0.0 {
            return Err(Throw::msg("`range` step can't be 0"));
        }
        let count = ((end - start) / step).ceil().max(0.0);
        if count > c.limits().max_values as f64 {
            return Err(Throw::Limit(format!("`range` would make {count} numbers — too many")));
        }
        Ok(Value::list((0..count as usize).map(|i| Value::Num(start + step * i as f64)).collect()))
    }));
    func(vm, &["format"], Rc::new(|_, a| Ok(Value::Str(format(a.str(0)?, &a.values[1..])?.into()))));
    func(vm, &["random"], Rc::new(random_value));
    func(vm, &["choose"], Rc::new(|c, a| {
        let l = a.list(0)?;
        if l.is_empty() {
            return Ok(Value::Nil);
        }
        let i = c.rng().int(0, l.len() as i64 - 1) as usize;
        Ok(l.get(i).unwrap_or_default())
    }));
    func(vm, &["seed"], Rc::new(|c, a| {
        let n = a.num(0)?;
        c.rng().seed(n.to_bits());
        Ok(Value::Nil)
    }));
    func(vm, &["vec2"], Rc::new(|_, a| Ok(Value::Vec2(a.opt_num(0, 0.0)? as f32, a.opt_num(1, 0.0)? as f32))));
    func(vm, &["vec3"], Rc::new(|_, a| {
        Ok(Value::Vec3(a.opt_num(0, 0.0)? as f32, a.opt_num(1, 0.0)? as f32, a.opt_num(2, 0.0)? as f32))
    }));
    func(vm, &["color", "rgba", "rgb"], Rc::new(|_, a| {
        if let Value::Str(s) = a.get(0) {
            return color_from_hex(s).ok_or_else(|| Throw::msg(format!("`{s}` is not a color — write \"#ff8800\" or \"#ff8800cc\"")));
        }
        Ok(Value::color(a.num(0)? as f32, a.num(1)? as f32, a.num(2)? as f32, a.opt_num(3, 1.0)? as f32))
    }));
    func(vm, &["__check_fn"], Rc::new(|_, a| match a.get(0) {
        Value::Func(_) | Value::Native(_) => Ok(Value::Nil),
        other => Err(Throw::msg(format!(
            "bad argument to `{}`: expected a function, got {}",
            a.get(1).to_text(),
            other.describe()
        ))),
    }));
    func(vm, &["__replace"], Rc::new(|_, a| {
        let target = a.list(0)?;
        let src = a.list(1)?;
        target.replace(src.to_vec());
        Ok(Value::Nil)
    }));
    func(vm, &["__sort_native"], Rc::new(|_, a| {
        let l = a.list(0)?;
        let items = l.to_vec();
        let all_num = items.iter().all(|v| matches!(v, Value::Num(_)));
        let all_str = items.iter().all(|v| matches!(v, Value::Str(_)));
        if !(all_num || all_str) {
            return Err(Throw::msg(
                "`sort` without a comparator works on lists of numbers or lists of strings — pass a function: `list.sort((a, b) => a.score < b.score)`",
            ));
        }
        l.modify(|v| {
            if all_num {
                v.sort_by(|a, b| a.as_num().unwrap_or(0.0).total_cmp(&b.as_num().unwrap_or(0.0)));
            } else {
                v.sort_by(|a, b| a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")));
            }
        });
        Ok(a.get(0).clone())
    }));
    // Короткие математические функции и без `math.`.
    func(vm, &["min"], Rc::new(|_, a| Ok(Value::Num(num_list(&a)?.into_iter().fold(f64::INFINITY, f64::min)))));
    func(vm, &["max"], Rc::new(|_, a| Ok(Value::Num(num_list(&a)?.into_iter().fold(f64::NEG_INFINITY, f64::max)))));
    func(vm, &["abs"], Rc::new(|_, a| Ok(Value::Num(a.num(0)?.abs()))));
    func(vm, &["floor"], Rc::new(|_, a| Ok(Value::Num(a.num(0)?.floor()))));
    func(vm, &["round"], Rc::new(|_, a| Ok(Value::Num((a.num(0)? + 0.5).floor()))));
    func(vm, &["clamp"], Rc::new(|_, a| {
        let (x, lo, hi) = (a.num(0)?, a.num(1)?, a.num(2)?);
        Ok(Value::Num(x.max(lo).min(hi)))
    }));
    func(vm, &["lerp"], Rc::new(lerp));
}

fn lerp(_: &mut NativeCtx, a: Args) -> NativeResult {
    let t = a.num(2)?;
    match (a.get(0), a.get(1)) {
        (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x + (y - x) * t)),
        (p @ (Value::Vec2(..) | Value::Vec3(..)), q) => {
            let (u, v) = (vec_of(p).unwrap_or_default(), vec_of(q).ok_or_else(|| a.bad(1, "a vector"))?);
            let t = t as f32;
            Ok(same_kind(p, [u[0] + (v[0] - u[0]) * t, u[1] + (v[1] - u[1]) * t, u[2] + (v[2] - u[2]) * t]))
        }
        (Value::Color(c), Value::Color(d)) => {
            let t = t as f32;
            Ok(Value::color(c[0] + (d[0] - c[0]) * t, c[1] + (d[1] - c[1]) * t, c[2] + (d[2] - c[2]) * t, c[3] + (d[3] - c[3]) * t))
        }
        _ => Err(a.bad(0, "a number, vector or color")),
    }
}

fn install_math(vm: &mut Vm) {
    let mut m = OrderedMap::default();
    type Unary = (&'static str, fn(f64) -> f64);
    let unary: [Unary; 16] = [
        ("floor", f64::floor),
        ("ceil", f64::ceil),
        ("round", |x| (x + 0.5).floor()),
        ("trunc", f64::trunc),
        ("abs", f64::abs),
        ("sqrt", f64::sqrt),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("exp", f64::exp),
        ("sign", |x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { 0.0 }),
        ("deg", f64::to_degrees),
        ("rad", f64::to_radians),
        ("fract", |x| x - x.floor()),
    ];
    for (name, f) in unary {
        ns_fn(&mut m, name, Rc::new(move |_, a| Ok(Value::Num(f(a.num(0)?)))));
    }
    ns_fn(&mut m, "atan", Rc::new(|_, a| {
        Ok(Value::Num(if a.has(1) { a.num(0)?.atan2(a.num(1)?) } else { a.num(0)?.atan() }))
    }));
    ns_fn(&mut m, "atan2", Rc::new(|_, a| Ok(Value::Num(a.num(0)?.atan2(a.num(1)?)))));
    ns_fn(&mut m, "pow", Rc::new(|_, a| Ok(Value::Num(a.num(0)?.powf(a.num(1)?)))));
    ns_fn(&mut m, "log", Rc::new(|_, a| {
        let x = a.num(0)?;
        Ok(Value::Num(if a.has(1) { x.log(a.num(1)?) } else { x.ln() }))
    }));
    ns_fn(&mut m, "min", Rc::new(|_, a| Ok(Value::Num(num_list(&a)?.into_iter().fold(f64::INFINITY, f64::min)))));
    ns_fn(&mut m, "max", Rc::new(|_, a| Ok(Value::Num(num_list(&a)?.into_iter().fold(f64::NEG_INFINITY, f64::max)))));
    ns_fn(&mut m, "clamp", Rc::new(|_, a| {
        let (x, lo, hi) = (a.num(0)?, a.num(1)?, a.num(2)?);
        Ok(Value::Num(x.max(lo).min(hi)))
    }));
    ns_fn(&mut m, "lerp", Rc::new(lerp));
    ns_fn(&mut m, "inverse_lerp", Rc::new(|_, a| {
        let (lo, hi, x) = (a.num(0)?, a.num(1)?, a.num(2)?);
        Ok(Value::Num(if hi == lo { 0.0 } else { (x - lo) / (hi - lo) }))
    }));
    ns_fn(&mut m, "random", Rc::new(random_value));
    ns_fn(&mut m, "randomseed", Rc::new(|c, a| {
        let n = a.num(0)?;
        c.rng().seed(n.to_bits());
        Ok(Value::Nil)
    }));
    m.insert(Key::Str("pi".into()), Value::Num(std::f64::consts::PI));
    m.insert(Key::Str("tau".into()), Value::Num(std::f64::consts::TAU));
    m.insert(Key::Str("huge".into()), Value::Num(f64::INFINITY));
    m.insert(Key::Str("inf".into()), Value::Num(f64::INFINITY));
    vm.register_value("math", Value::Map(Map::new(m)));
}

fn install_strings(vm: &mut Vm) {
    let mut ns = OrderedMap::default();
    let s = TypeTag::Str;
    both(vm, &mut ns, s, &["len"], Rc::new(|_, a| Ok(Value::Num(char_len(a.str(0)?) as f64))));
    both(vm, &mut ns, s, &["upper", "toUpperCase"], Rc::new(|_, a| Ok(Value::Str(a.str(0)?.to_uppercase().into()))));
    both(vm, &mut ns, s, &["lower", "toLowerCase"], Rc::new(|_, a| Ok(Value::Str(a.str(0)?.to_lowercase().into()))));
    both(vm, &mut ns, s, &["sub", "slice", "substring"], Rc::new(|_, a| {
        let c = chars(a.str(0)?);
        let start = rel_index(a.opt_num(1, 0.0)?, c.len());
        let end = rel_index(a.opt_num(2, c.len() as f64)?, c.len());
        Ok(Value::Str(if start < end { c[start..end].iter().collect::<String>().into() } else { "".into() }))
    }));
    both(vm, &mut ns, s, &["find"], Rc::new(|_, a| {
        let hay = a.str(0)?;
        let needle = a.str(1)?;
        let from = a.opt_num(2, 0.0)?.max(0.0) as usize;
        let c = chars(hay);
        let byte_from: usize = c.iter().take(from).map(|ch| ch.len_utf8()).sum();
        Ok(match hay.get(byte_from..).and_then(|rest| rest.find(&**needle)) {
            Some(b) => Value::Num((from + hay[byte_from..byte_from + b].chars().count()) as f64),
            None => Value::Nil,
        })
    }));
    both(vm, &mut ns, s, &["indexOf", "index_of"], Rc::new(|_, a| {
        let hay = a.str(0)?;
        let needle = a.str(1)?;
        Ok(Value::Num(match hay.find(&**needle) {
            Some(b) => hay[..b].chars().count() as f64,
            None => -1.0,
        }))
    }));
    both(vm, &mut ns, s, &["contains", "includes"], Rc::new(|_, a| Ok(Value::Bool(a.str(0)?.contains(&**a.str(1)?)))));
    both(vm, &mut ns, s, &["startsWith", "starts_with"], Rc::new(|_, a| Ok(Value::Bool(a.str(0)?.starts_with(&**a.str(1)?)))));
    both(vm, &mut ns, s, &["endsWith", "ends_with"], Rc::new(|_, a| Ok(Value::Bool(a.str(0)?.ends_with(&**a.str(1)?)))));
    both(vm, &mut ns, s, &["split"], Rc::new(|_, a| {
        let text = a.str(0)?;
        let parts: Vec<Value> = match a.opt_str(1)? {
            None => text.split_whitespace().map(Value::from).collect(),
            Some(sep) if sep.is_empty() => text.chars().map(|c| Value::Str(c.to_string().into())).collect(),
            Some(sep) => text.split(&**sep).map(Value::from).collect(),
        };
        Ok(Value::list(parts))
    }));
    both(vm, &mut ns, s, &["replace", "replaceAll", "gsub"], Rc::new(|c, a| {
        let from = a.str(1)?;
        if from.is_empty() {
            return Err(Throw::msg("`replace` needs a non-empty string to look for"));
        }
        let out = a.str(0)?.replace(&**from, &a.get(2).to_text());
        check_str(c.vm_ref(), &out)?;
        Ok(Value::Str(out.into()))
    }));
    both(vm, &mut ns, s, &["trim"], Rc::new(|_, a| Ok(Value::str(a.str(0)?.trim()))));
    both(vm, &mut ns, s, &["trimStart", "trim_start"], Rc::new(|_, a| Ok(Value::str(a.str(0)?.trim_start()))));
    both(vm, &mut ns, s, &["trimEnd", "trim_end"], Rc::new(|_, a| Ok(Value::str(a.str(0)?.trim_end()))));
    both(vm, &mut ns, s, &["rep", "repeat"], Rc::new(|c, a| {
        let n = a.int(1)?.max(0) as usize;
        let src = a.str(0)?;
        if src.len().saturating_mul(n) > c.limits().max_string_len {
            return Err(Throw::Limit(format!("string too long: more than {} bytes", c.limits().max_string_len)));
        }
        Ok(Value::Str(src.repeat(n).into()))
    }));
    both(vm, &mut ns, s, &["format"], Rc::new(|_, a| Ok(Value::Str(format(a.str(0)?, &a.values[1..])?.into()))));
    both(vm, &mut ns, s, &["chars"], Rc::new(|_, a| {
        Ok(Value::list(a.str(0)?.chars().map(|c| Value::Str(c.to_string().into())).collect()))
    }));
    both(vm, &mut ns, s, &["reverse"], Rc::new(|_, a| Ok(Value::Str(a.str(0)?.chars().rev().collect::<String>().into()))));
    both(vm, &mut ns, s, &["at", "charAt"], Rc::new(|_, a| {
        let c = chars(a.str(0)?);
        let i = a.num(1)?;
        let i = if i < 0.0 { c.len() as f64 + i } else { i };
        Ok(if i >= 0.0 && (i as usize) < c.len() { Value::Str(c[i as usize].to_string().into()) } else { Value::Nil })
    }));
    both(vm, &mut ns, s, &["padStart", "pad_start"], Rc::new(|_, a| pad(a, true)));
    both(vm, &mut ns, s, &["padEnd", "pad_end"], Rc::new(|_, a| pad(a, false)));
    vm.register_value("string", Value::Map(Map::new(ns)));
    method(vm, TypeTag::Num, &["toFixed", "to_fixed"], Rc::new(|_, a| {
        let digits = a.opt_num(1, 0.0)?.clamp(0.0, 20.0) as usize;
        Ok(Value::Str(format!("{:.*}", digits, a.num(0)?).into()))
    }));
}

fn pad(a: Args, start: bool) -> NativeResult {
    let text = a.str(0)?;
    let width = a.int(1)?.max(0) as usize;
    let fill = match a.opt_str(2)? {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => " ".into(),
    };
    let len = char_len(text);
    if len >= width || width > 1 << 16 {
        return Ok(Value::Str(text.clone()));
    }
    let padding: String = fill.chars().cycle().take(width - len).collect();
    Ok(Value::Str(if start { format!("{padding}{text}") } else { format!("{text}{padding}") }.into()))
}

fn install_lists(vm: &mut Vm) {
    let l = TypeTag::List;
    let mut table = OrderedMap::default();
    method(vm, l, &["push", "append", "add"], Rc::new(|_, a| {
        let list = a.list(0)?;
        for v in &a.values[1..] {
            list.push(v.clone());
        }
        Ok(Value::Num(list.len() as f64))
    }));
    method(vm, l, &["pop"], Rc::new(|_, a| Ok(a.list(0)?.pop().unwrap_or_default())));
    method(vm, l, &["shift"], Rc::new(|_, a| Ok(a.list(0)?.remove(0).unwrap_or_default())));
    method(vm, l, &["unshift"], Rc::new(|_, a| {
        let list = a.list(0)?;
        for (i, v) in a.values[1..].iter().enumerate() {
            list.insert(i, v.clone());
        }
        Ok(Value::Num(list.len() as f64))
    }));
    let insert: F = Rc::new(|_, a| {
        let list = a.list(0)?;
        if a.len() <= 2 {
            list.push(a.get(1).clone());
            return Ok(Value::Nil);
        }
        let i = a.int(1)?;
        if i < 0 || !list.insert(i as usize, a.get(2).clone()) {
            return Err(Throw::msg(format!("`insert`: index {i} is out of range for a list of length {}", list.len())));
        }
        Ok(Value::Nil)
    });
    method(vm, l, &["insert"], insert.clone());
    ns_fn(&mut table, "insert", insert);
    let remove: F = Rc::new(|_, a| {
        let list = a.list(0)?;
        if !a.has(1) {
            return Ok(list.pop().unwrap_or_default());
        }
        let i = a.int(1)?;
        if i < 0 {
            return Ok(Value::Nil);
        }
        Ok(list.remove(i as usize).unwrap_or_default())
    });
    method(vm, l, &["remove"], remove.clone());
    ns_fn(&mut table, "remove", remove);
    method(vm, l, &["len", "size"], Rc::new(|_, a| Ok(Value::Num(a.list(0)?.len() as f64))));
    method(vm, l, &["includes", "contains", "has"], Rc::new(|_, a| {
        let needle = a.get(1);
        Ok(Value::Bool(a.list(0)?.borrow().iter().any(|v| v.equals(needle))))
    }));
    method(vm, l, &["indexOf", "index_of"], Rc::new(|_, a| {
        let needle = a.get(1);
        Ok(Value::Num(a.list(0)?.borrow().iter().position(|v| v.equals(needle)).map(|i| i as f64).unwrap_or(-1.0)))
    }));
    method(vm, l, &["reverse"], Rc::new(|_, a| {
        a.list(0)?.modify(|v| v.reverse());
        Ok(a.get(0).clone())
    }));
    let join: F = Rc::new(|c, a| {
        let sep = match a.opt_str(1)? {
            Some(s) => s.to_string(),
            None => ",".into(),
        };
        let out = a.list(0)?.borrow().iter().map(Value::to_text).collect::<Vec<_>>().join(&sep);
        check_str(c.vm_ref(), &out)?;
        Ok(Value::Str(out.into()))
    });
    method(vm, l, &["join"], join.clone());
    ns_fn(&mut table, "concat", join);
    method(vm, l, &["slice"], Rc::new(|_, a| {
        let list = a.list(0)?;
        let n = list.len();
        let start = rel_index(a.opt_num(1, 0.0)?, n);
        let end = rel_index(a.opt_num(2, n as f64)?, n);
        let items = list.borrow();
        Ok(Value::list(if start < end { items[start..end].to_vec() } else { Vec::new() }))
    }));
    method(vm, l, &["concat"], Rc::new(|_, a| {
        let mut out = a.list(0)?.to_vec();
        for (i, v) in a.values.iter().enumerate().skip(1) {
            match v {
                Value::List(o) => out.extend(o.to_vec()),
                _ => return Err(a.bad(i, "a list")),
            }
        }
        Ok(Value::list(out))
    }));
    method(vm, l, &["copy"], Rc::new(|_, a| Ok(Value::list(a.list(0)?.to_vec()))));
    method(vm, l, &["clear"], Rc::new(|_, a| {
        a.list(0)?.replace(Vec::new());
        Ok(Value::Nil)
    }));
    method(vm, l, &["first"], Rc::new(|_, a| Ok(a.list(0)?.get(0).unwrap_or_default())));
    method(vm, l, &["last"], Rc::new(|_, a| {
        let list = a.list(0)?;
        Ok(list.len().checked_sub(1).and_then(|i| list.get(i)).unwrap_or_default())
    }));
    method(vm, l, &["shuffle"], Rc::new(|c, a| {
        let list = a.list(0)?;
        let mut items = list.to_vec();
        for i in (1..items.len()).rev() {
            let j = c.rng().int(0, i as i64) as usize;
            items.swap(i, j);
        }
        list.replace(items);
        Ok(a.get(0).clone())
    }));
    method(vm, l, &["sum"], Rc::new(|_, a| {
        let mut s = 0.0;
        for v in a.list(0)?.borrow().iter() {
            s += v.as_num().ok_or_else(|| Throw::msg(format!("`sum` works on numbers, got {}", v.describe())))?;
        }
        Ok(Value::Num(s))
    }));
    vm.register_value("table", Value::Map(Map::new(table)));
}

fn install_maps(vm: &mut Vm) {
    let m = TypeTag::Map;
    let key = |a: &Args, i: usize| Key::from_value(a.get(i)).map_err(Throw::Msg);
    method(vm, m, &["keys"], Rc::new(|_, a| Ok(Value::list(a.map(0)?.borrow().iter().map(|(k, _)| k.to_value()).collect()))));
    method(vm, m, &["values"], Rc::new(|_, a| Ok(Value::list(a.map(0)?.borrow().iter().map(|(_, v)| v.clone()).collect()))));
    method(vm, m, &["entries"], Rc::new(|_, a| {
        Ok(Value::list(a.map(0)?.borrow().iter().map(|(k, v)| Value::list(vec![k.to_value(), v.clone()])).collect()))
    }));
    method(vm, m, &["has", "contains"], Rc::new(move |_, a| Ok(Value::Bool(a.map(0)?.borrow().contains(&key(&a, 1)?)))));
    method(vm, m, &["get"], Rc::new(move |_, a| {
        Ok(a.map(0)?.borrow().get(&key(&a, 1)?).cloned().unwrap_or_else(|| a.get(2).clone()))
    }));
    method(vm, m, &["set"], Rc::new(move |_, a| {
        let k = key(&a, 1)?;
        let v = a.get(2).clone();
        let map = a.map(0)?;
        if v.is_nil() {
            map.borrow_mut().remove(&k);
        } else {
            map.borrow_mut().insert(k, v);
        }
        Ok(a.get(0).clone())
    }));
    method(vm, m, &["remove", "delete"], Rc::new(move |_, a| Ok(a.map(0)?.borrow_mut().remove(&key(&a, 1)?).unwrap_or_default())));
    method(vm, m, &["size", "len", "length"], Rc::new(|_, a| Ok(Value::Num(a.map(0)?.borrow().len() as f64))));
    method(vm, m, &["clear"], Rc::new(|_, a| {
        a.map(0)?.borrow_mut().clear();
        Ok(Value::Nil)
    }));
    method(vm, m, &["copy"], Rc::new(|_, a| {
        let mut out = OrderedMap::default();
        for (k, v) in a.map(0)?.borrow().iter() {
            out.insert(k.clone(), v.clone());
        }
        Ok(Value::Map(Map::new(out)))
    }));
}

fn install_vectors(vm: &mut Vm) {
    for tag in [TypeTag::Vec2, TypeTag::Vec3] {
        method(vm, tag, &["length", "magnitude", "len"], Rc::new(|_, a| {
            let v = vec_of(a.get(0)).ok_or_else(|| a.bad(0, "a vector"))?;
            Ok(Value::Num(((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f64).sqrt()))
        }));
        method(vm, tag, &["normalized", "normalize"], Rc::new(|_, a| {
            let v = vec_of(a.get(0)).ok_or_else(|| a.bad(0, "a vector"))?;
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let v = if len > 0.0 { [v[0] / len, v[1] / len, v[2] / len] } else { v };
            Ok(same_kind(a.get(0), v))
        }));
        method(vm, tag, &["dot"], Rc::new(|_, a| {
            let u = vec_of(a.get(0)).ok_or_else(|| a.bad(0, "a vector"))?;
            let v = vec_of(a.get(1)).ok_or_else(|| a.bad(1, "a vector"))?;
            Ok(Value::Num((u[0] * v[0] + u[1] * v[1] + u[2] * v[2]) as f64))
        }));
        method(vm, tag, &["distance", "distance_to"], Rc::new(|_, a| {
            let u = vec_of(a.get(0)).ok_or_else(|| a.bad(0, "a vector"))?;
            let v = vec_of(a.get(1)).ok_or_else(|| a.bad(1, "a vector"))?;
            let d = [u[0] - v[0], u[1] - v[1], u[2] - v[2]];
            Ok(Value::Num(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64).sqrt()))
        }));
        method(vm, tag, &["lerp"], Rc::new(lerp));
    }
    method(vm, TypeTag::Vec3, &["cross"], Rc::new(|_, a| {
        let u = vec_of(a.get(0)).ok_or_else(|| a.bad(0, "a vector"))?;
        let v = vec_of(a.get(1)).ok_or_else(|| a.bad(1, "a vector"))?;
        Ok(Value::Vec3(u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]))
    }));
    method(vm, TypeTag::Color, &["lerp"], Rc::new(lerp));
    method(vm, TypeTag::Color, &["with_alpha", "withAlpha"], Rc::new(|_, a| match a.get(0) {
        Value::Color(c) => Ok(Value::color(c[0], c[1], c[2], a.num(1)? as f32)),
        _ => Err(a.bad(0, "a color")),
    }));
}

fn install_tasks(vm: &mut Vm) {
    func(vm, &["wait", "sleep"], Rc::new(|c, a| {
        let s = a.opt_num(0, 0.0)?;
        if !s.is_finite() || s < 0.0 {
            return Err(a.bad(0, "a number of seconds ≥ 0"));
        }
        c.suspend(Suspend::Seconds(s));
        Ok(Value::Nil)
    }));
    func(vm, &["wait_frame", "wait_frames"], Rc::new(|c, a| {
        let n = a.opt_num(0, 1.0)?.max(1.0).min(u32::MAX as f64) as u32;
        c.suspend(Suspend::Frames(n));
        Ok(Value::Nil)
    }));
    func(vm, &["wait_message"], Rc::new(|c, a| {
        let name = a.str(0)?.clone();
        c.suspend(Suspend::Message(name));
        Ok(Value::Nil)
    }));
    let wait_task: F = Rc::new(|c, a| {
        let id = a.handle(0, TASK_CLASS, "a task")?;
        if c.is_running(id) && id != c.task_id() {
            c.suspend(Suspend::Task(id));
        }
        Ok(Value::Nil)
    });
    func(vm, &["wait_task"], wait_task.clone());
    func(vm, &["start", "spawn"], Rc::new(|c, a| {
        let f = a.func(0)?.clone();
        c.spawn(f, a.values[1..].to_vec())
    }));
    let stop: F = Rc::new(|c, a| {
        let id = if a.has(0) { a.handle(0, TASK_CLASS, "a task")? } else { c.task_id() };
        c.stop(id);
        Ok(Value::Nil)
    });
    func(vm, &["stop"], stop.clone());
    func(vm, &["stop_all"], Rc::new(|c, _| {
        c.stop_others();
        Ok(Value::Nil)
    }));
    func(vm, &["on"], Rc::new(|c, a| {
        let name = a.str(0)?.clone();
        let f = a.func(1)?.clone();
        if !matches!(f, Value::Func(_)) {
            return Err(a.bad(1, "a script function"));
        }
        c.on(name, f);
        Ok(Value::Nil)
    }));
    func(vm, &["off"], Rc::new(|c, a| {
        let name = a.str(0)?.clone();
        c.off(&name);
        Ok(Value::Nil)
    }));
    func(vm, &["emit"], Rc::new(|c, a| {
        let name = a.str(0)?.clone();
        c.post(name, a.get(1).clone());
        Ok(Value::Nil)
    }));
    func(vm, &["time"], Rc::new(|c, _| Ok(Value::Num(c.group_time().0))));
    func(vm, &["dt"], Rc::new(|c, _| Ok(Value::Num(c.group_time().1))));
    vm.class_method(TASK_CLASS, "stop", move |c, a| stop(c, a));
    vm.class_method(TASK_CLASS, "wait", move |c, a| wait_task(c, a));
}

/// Пары «имя в прелюдии → куда поставить».
const PRELUDE_LIST: &[(&str, &[&str])] = &[
    ("__list_map", &["map"]),
    ("__list_filter", &["filter"]),
    ("__list_find", &["find"]),
    ("__list_find_index", &["find_index", "findIndex"]),
    ("__list_each", &["each", "forEach", "for_each"]),
    ("__list_reduce", &["reduce"]),
    ("__list_some", &["some", "any"]),
    ("__list_every", &["every", "all"]),
    ("__list_count", &["count"]),
    ("__list_sort", &["sort"]),
];

fn install_prelude(vm: &mut Vm) {
    let source = include_str!("prelude.scenario");
    let script = match compile_script("<prelude>", source, true) {
        Ok(s) => s,
        Err(e) => panic!("Scenario prelude does not compile: {e}"),
    };
    let inst = match vm.instantiate(&script) {
        Ok(i) => i,
        Err(e) => panic!("Scenario prelude does not link: {e}"),
    };
    let mut task = Task::new(&inst.main, Vec::new()).expect("prelude main");
    let mut sched = SchedCtx::default();
    match task.resume(vm, &mut (), &mut sched, Value::Nil) {
        Ok(Status::Done(_)) => {}
        other => panic!("Scenario prelude did not finish: {:?}", other.err()),
    }
    let get = |name: &str| inst.module.get(name).unwrap_or_default();
    for (src, names) in PRELUDE_LIST {
        let f = get(src);
        for n in *names {
            vm.method_value(TypeTag::List, n, f.clone());
        }
    }
    vm.method_value(TypeTag::Map, "each", get("__map_each"));
    vm.method_value(TypeTag::Map, "forEach", get("__map_each"));
    if let Some(Value::Map(t)) = vm.builtins.get("table").cloned() {
        t.borrow_mut().insert(Key::Str("sort".into()), get("__list_sort"));
    }
    vm.register_value("pcall", get("pcall"));
    vm.register_value("wait_until", get("wait_until"));
}

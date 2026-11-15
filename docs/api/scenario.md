# Scenario — язык сценариев

**Scenario** — маленький встроенный язык движка для *сценариев*: катсцены, обучение, диалоги,
события уровня, волны врагов, «клей» между компонентами. Компоненты по-прежнему пишутся на Rust и
C++; сценарий работает поверх них, ждёт (`wait(1)`) посреди циклов и перезапускается сам, как
только файл сохранён — без пересборки и без перезапуска редактора.

Синтаксис — смесь **Lua** и **JavaScript**: обе записи принимаются и смешиваются в одном файле.
Кто знает любой из двух языков, может писать сразу.

```
-- Lua-форма                                 // JS-форма
local hp = 100                               let hp = 100
function hit(n)                              function hit(n) {
    hp = hp - n                                  hp -= n
end                                          }
if hp <= 0 then die() end                    if (hp <= 0) { die() }
```

Файлы: `assets/scenarios/*.scenario`. Запускает их компонент `ScenarioRunner`.

* [Учебник: 10 примеров](#учебник-10-примеров)
* [Справочник языка](#справочник-языка)
* [Таблица «Lua ↔ JS»](#таблица-lua--js)
* [Чем Scenario отличается от Lua и от JS](#чем-scenario-отличается-от-lua-и-от-js)
* [Стандартная библиотека](#стандартная-библиотека)
* [API движка](#api-движка)
* [Из Rust](#из-rust)
* [Пределы](#пределы)
* [Ошибки](#ошибки)
* [Горячая перезагрузка](#горячая-перезагрузка)
* [Когда Scenario, а когда Rust / C++](#когда-scenario-а-когда-rust--c)

---

## Учебник: 10 примеров

Каждый пример — целый файл `.scenario`. Повесьте на объект `ScenarioRunner` (в редакторе:
Add Component → New Scenario Script…, или в коде `ScenarioRunner::new("scenarios/x.scenario")`)
и нажмите Play.

**1. Привет.** `log` пишет в Console (с именем объекта), `self` — объект, на котором висит сценарий.

```
log("Привет от " .. self.name)
```

**2. Ожидание.** `wait(секунды)` останавливает только этот сценарий, игра идёт дальше.

```
for i = 3, 1, -1 do
    ui.text("Hint", i .. "…")
    wait(1)
end
ui.text("Hint", "Лови!")
```

**3. Движение.** `move_to` ждёт, пока объект доедет; `tween` анимирует любое поле и не ждёт.

```
const door = find("Door")
door.tween("Light.intensity", 0, 0.5)
door.move_to(0, 3, 1.5, "out_back")      -- x, y, секунды, сглаживание
log("дверь открыта")
```

**4. Параллельно.** `start(fn)` запускает функцию отдельной задачей.

```
start(() => {
    while (true) {
        find("Lamp").tween("Light.color", color("#3080ff"), 1)
        wait(1)
        find("Lamp").tween("Light.color", color("#ff5010"), 1)
        wait(1)
    }
})
wait(5)
log("лампа мигает уже 5 секунд")
```

**5. Сообщения.** `on(имя, fn)` — обработчик (сам может ждать), `wait_message` ждёт одно.
Сообщения шлют Rust (`ctx.send_message(obj, "hit", Value::Int(3))`), C++, кнопки интерфейса и
другие сценарии (`obj.send("hit", 3)`).

```
let hp = 10
on("hit", (dmg) => {
    hp -= dmg
    if (hp <= 0) emit("died")
})
const key = wait_message("key")
log("открыто ключом " .. key)
```

**6. Ввод.** Проверяйте каждый кадр с `wait_frame()`.

```
while not key_down("space") do
    wait_frame()
end
log("пробел нажат")
```

**7. Интерфейс.** Объекты интерфейса находятся по имени.

```
ui.show("Dialog")
ui.text("DialogText", "Нажмите «Дальше»")
wait_click("next")                -- имя кнопки (Button.message или имя объекта)
ui.hide("Dialog")
```

**8. Списки и словари.** Списки с нуля; методы как в JS.

```
const enemies = find_all_tag("Enemy")
const names = enemies.map((e) => e.name).filter((n) => n.startsWith("Orc"))
let stats = { killed = 0, wave: 1 }
for (const [key, value] of stats) log(key, value)
log(`орков: ${names.length}`)
```

**9. Волны врагов.** Создание, ожидание, уничтожение.

```
for wave = 1, 3 do
    ui.text("Wave", "Волна " .. wave)
    for i = 1, wave * 2 do
        spawn("enemy", random(-5, 5), 6)       -- prefabs/enemy.prefab
        wait(0.4)
    end
    wait_until(() => find_all_tag("Enemy").length == 0)
end
play("victory")
```

**10. Ошибки под контролем.** `try / catch` (или `pcall`) ловит ошибку и сценарий продолжает.

```
try {
    find("Boss").move_to(0, 0, 2)
} catch (e) {
    warn("босса нет: " + e)
}
local ok, err = pcall(function() error("упс") end)
```

---

## Справочник языка

### Комментарии

| | |
|---|---|
| `// строка`, `-- строка` | до конца строки |
| `/* блок */`, `--[[ блок ]]` | многострочный |

`--` **всегда** начинает комментарий, поэтому в Scenario нет `x--` (пишите `x -= 1`). Компилятор
замечает `i--` и подсказывает.

### Значения

| Тип (`typeof`) | Примеры | Замечания |
|---|---|---|
| `nil` | `nil`, `null` | |
| `boolean` | `true`, `false` | |
| `number` | `42`, `3.5`, `1e3`, `0xff`, `1_000` | f64; целые печатаются без `.0` |
| `string` | `"a"`, `'b'`, `` `c ${x}` `` | UTF-8, индексы — по символам |
| `list` | `[1, 2, 3]`, `{1, 2, 3}` | индексы **с 0** |
| `map` | `{ name = "Анна", age: 30, ["k"] = 1 }` | порядок вставки |
| `function` | `function() … end`, `(x) => x * 2` | замыкания |
| `vec2`, `vec3` | `vec2(1, 2)`, `vec3(1, 2, 3)` | значения (копируются); поля `.x .y .z` |
| `color` | `color(1, 0.5, 0)`, `color("#ff8800")` | значение; поля `.r .g .b .a` |
| `object` | `self`, `find("Door")` | объект сцены (ручка) |
| `task` | `start(fn)` | задача; `.running`, `.done`, `:stop()`, `:wait()` |

Строки: экранирование `\n \t \r \\ \" \' \` \$ \0 \xFF \u{1F600}`. Обычная строка не может
переноситься на новую строку; шаблонная (`` `…` ``) — может, внутри `${выражение}`.

### Переменные

| | |
|---|---|
| `let x = 1`, `local x = 1`, `var x = 1` | одно и то же: переменная блока |
| `const MAX = 3` | нельзя присвоить заново (ошибка компиляции) |
| `local a, b = 1, 2` | несколько |
| `let a = 1, b = 2` | несколько (JS-форма) |
| `let x` | значение `nil` |
| `let [a, b] = list` | разбор списка |
| `local ok, res = pcall(f)` | одно значение-список на несколько имён — разбирается |
| `a, b = b, a` | множественное присваивание |
| `x += 1  -= *= /= %= ..=` и `x++` | `x++` — только отдельной инструкцией |

Переменные **объявляются**: чтение необъявленного имени — ошибка до запуска с подсказкой
(`` `scroe` is not defined — did you mean `score`? ``), присваивание необъявленному — ошибка
компиляции. Переменные видны в своём блоке; объявления верхнего уровня файла — общие для всех
задач этого сценария. Объявления `function` поднимаются в начало блока (их можно вызвать выше по
тексту). Чтение `let` до строки объявления — ошибка.

### Функции

```
function add(a, b) { return a + b }        -- JS-тело
function add(a, b) return a + b end        -- Lua-тело
local function helper() … end
const twice = (x) => x * 2                 -- стрелка с выражением
const log2 = (x) => { log(x); log(x) }     -- стрелка с блоком
const inc = x => x + 1
local f = function(x) return x end
function sum(first, ...rest) … end         -- остальные аргументы списком
sum(...list)                               -- список как аргументы
function M.make() … end                    -- поле словаря
function M:hello() return self.name end    -- метод: неявный параметр self
```

Возвращается одно значение (`return a` или `return [a, b]`). Недостающие аргументы — `nil`,
лишние отбрасываются. `obj:method(x)` передаёт `obj` как `self`; `obj.method(x)` у словаря
вызывает функцию из поля без `self`. У встроенных типов и объектов игры обе формы одинаковы:
`list.push(1)` == `list:push(1)`, `door.move_to(…)` == `door:move_to(…)`.

### Условия, циклы

```
if (a) { … } else if (b) { … } else { … }
if a then … elseif b then … else … end
if (a) return                              -- одна инструкция после условия

while (i < 10) { i += 1 }        while i < 10 do i = i + 1 end
do { step() } while (!done())    repeat step() until done()
do local tmp = 1 end             { let tmp = 1 }

for (let i = 0; i < 10; i++) { }           -- как в C/JS
for i = 0, 9 do end                        -- включительно
for i = 10, 1, -1 do end                   -- с шагом
for (const item of list) { }               for item of list do end
for (const [key, value] of map) { }        -- элемент словаря — пара [ключ, значение]
for i, item in ipairs(list) do end         -- индекс с 0 и значение
for key, value in pairs(map) do end
for (const key in map) { }                 -- in: ключ (у списка — индекс)
for (const ch of "строка") { }             -- символы
break    continue
```

Тело цикла: `{ … }`, `do … end` или одна инструкция. `of` даёт значения, `in` — ключи/индексы
(и значения, если имён два). Изменение списка во время обхода видно циклу.

### Ошибки в коде сценария

```
try { risky() } catch (e) { log(e) }       try risky() catch e log(e) end
throw "сообщение"      throw { code: 5 }  error("сообщение")
local ok, result = pcall(fn, arg1, arg2)   -- [true, результат] или [false, ошибка]
```

В `catch` приходит текст ошибки (для ошибок выполнения) или брошенное значение. Ошибки пределов
(бесконечная рекурсия, память) не ловятся.

### Операторы (по убыванию приоритета)

| Приоритет | Операторы | |
|---|---|---|
| 1 | `x.f  x[i]  f()  x:m()` | |
| 2 | `^`, `**` | степень, правая: `2^3^2 == 512`; `-2^2 == -4` |
| 3 | `-x  !x  not x  #x  typeof x` | |
| 4 | `*  /  %` | `%` — с округлением вниз: `-1 % 3 == 2` |
| 5 | `+  -` | `+` со строкой склеивает |
| 6 | `..` | склейка (правая), всё приводится к тексту |
| 7 | `<  <=  >  >=` | числа или строки |
| 8 | `==  ===  !=  !==  ~=` | без приведения типов |
| 9 | `&&  and` | возвращает операнд |
| 10 | `\|\|  or  ??` | `??` — только для `nil` |
| 11 | `c ? a : b` | правый |

`#x` и `x.length` — длина списка/строки (символы), `#map` — число пар. Векторы: `+ - * /`
покомпонентно, `vec * число`; цвета: `+ - *`, `color * число` (альфа не меняется).

### Перевод строки

Инструкция заканчивается переводом строки или `;` (можно и несколько на строке). Выражение
продолжается на следующей строке, если строка оборвалась на операторе (`+`, `and`, `,`, `(`…),
или следующая начинается с `.` или бинарного оператора:

```
let total = price +
    tax
const names = list
    .map((x) => x.name)
```

Но `(`, `[` и `` ` `` в **начале** строки никогда не приклеиваются к предыдущей строке (не
становятся вызовом или индексом) — это новая инструкция. Строка, которая считает значение и ничего
с ним не делает (`x`, `a == 1`), — ошибка компиляции.

---

## Таблица «Lua ↔ JS»

| Lua-форма | JS-форма |
|---|---|
| `-- комментарий`, `--[[ … ]]` | `// комментарий`, `/* … */` |
| `local x = 1` | `let x = 1`, `var x = 1` |
| `function f(a) … end` | `function f(a) { … }` |
| `function(x) return x * 2 end` | `(x) => x * 2` |
| `if c then … elseif d then … else … end` | `if (c) { … } else if (d) { … } else { … }` |
| `while c do … end` | `while (c) { … }` |
| `repeat … until c` | `do { … } while (!c)` |
| `for i = 0, 9 do … end` | `for (let i = 0; i < 10; i++) { … }` |
| `for i, v in ipairs(list) do … end` | `for (const v of list) { … }` |
| `for k, v in pairs(map) do … end` | `for (const [k, v] of map) { … }` |
| `and  or  not` | `&&  \|\|  !` |
| `~=` | `!=`, `!==` |
| `"a" .. "b"` | `"a" + "b"` |
| `#list` | `list.length` |
| `{ name = "x" }` | `{ name: "x" }` |
| `{1, 2, 3}` | `[1, 2, 3]` |
| `nil` | `null` |
| `obj:method()` | `obj.method()` |
| `pcall(f)` | `try { f() } catch (e) { }` |
| `string.format("%d", n)` | `` `${n}` `` |
| `table.insert(t, v)` | `t.push(v)` |

## Чем Scenario отличается от Lua и от JS

* **Списки с нуля** в обеих формах: `list[0]` — первый, `ipairs` даёт индексы с 0, `for i = a, b` —
  просто включительный счётчик.
* **Переменные объявляются**: нет «глобальных по умолчанию» (Lua) и нет `undefined` (JS):
  неизвестное имя — ошибка до запуска.
* **`--` — всегда комментарий**, нет `x--`.
* **Ложны** `nil`, `false`, `0`, `NaN` и `""` (как в JS; в Lua `0` и `""` истинны).
* `==` и `===` одинаковы и **без приведения**: `1 == "1"` — `false`.
* `{}` — пустой **словарь**; `{1, 2}` — список; смешивать ключи и элементы в одних `{}` нельзя.
* Чтение отсутствующего ключа словаря или индекса за концом списка — `nil`; присвоить `nil` в
  словарь — удалить ключ; запись в `list[#list]` добавляет в конец, дальше — ошибка.
* Нет классов, прототипов, метатаблиц, `this`, нескольких возвращаемых значений (возвращайте список),
  `goto`, побитовых операций и целочисленного `//` (это комментарий).
* `%` округляет вниз (как в Lua): `-1 % 3 == 2`. `/` — всегда дробное деление.
* Векторы и цвета — **значения**: `let w = v; w.x = 5` не меняет `v`. `obj.position.x += 1`
  работает: изменённый вектор записывается обратно в `obj.position`.
* Строки неизменяемы, индексы — по символам (`"Привет"[1] == "р"`).
* `import` других файлов нет: один сценарий — один файл.

---

## Стандартная библиотека

**Глобальные:** `print(...)` / `log(...)`, `warn(...)`, `type(x)` (= `typeof x`), `tostring(x)`,
`tonumber(s[, base])` (→ `nil`, если не число), `assert(cond[, msg])`, `error(value)`,
`pcall(f, ...)`, `len(x)`, `ipairs(list)`, `pairs(x)`, `keys(map)`, `values(map)`,
`range(n)` / `range(a, b[, step])` (конец не входит), `format(fmt, ...)`,
`random()` ([0, 1)), `random(n)` (целое 0…n-1), `random(a, b)` (целые — включительно, дробные — в
[a, b)), `choose(list)`, `seed(n)`, `vec2(x, y)`, `vec3(x, y, z)`, `color(r, g, b[, a])` /
`color("#rrggbb[aa]")`, `min`, `max`, `abs`, `floor`, `round`, `clamp(x, lo, hi)`,
`lerp(a, b, t)` (числа, векторы, цвета).

**Сопрограммы:** `wait(seconds)`, `wait_frame([n])`, `wait_until(fn[, timeout])` (→ `false` по
таймауту), `wait_message(name)` (→ значение), `wait_task(task)`, `start(fn, ...args)` (→ задача),
`stop([task])` (без аргумента — текущую), `stop_all()` (все остальные задачи и обработчики),
`on(name, fn)`, `off(name)`.

**`math`:** `floor ceil round trunc abs sqrt sin cos tan asin acos atan(y[, x]) atan2 exp
log(x[, base]) pow sign deg rad fract min max clamp lerp inverse_lerp random randomseed`,
константы `pi tau huge inf`. `round(2.5) == 3`, `round(-2.5) == -2` (как в JS).

**Строки** (методы, они же `string.имя(s, …)`): `len()`, `upper()`/`toUpperCase()`,
`lower()`/`toLowerCase()`, `sub(i[, j])` = `slice(i[, j])` = `substring` (с нуля, конец не входит,
отрицательные — с конца), `find(s[, from])` (→ индекс или `nil`), `indexOf(s)` / `index_of` (→ -1),
`contains`/`includes`, `startsWith`/`starts_with`, `endsWith`/`ends_with`, `split([sep])` (без
разделителя — по пробелам, `""` — по символам), `replace(a, b)` (все вхождения; = `replaceAll`,
`gsub`), `trim()`, `trimStart`, `trimEnd`, `rep(n)`/`repeat(n)`, `format(...)`, `chars()`,
`reverse()`, `at(i)`/`charAt(i)`, `padStart(n[, ch])`, `padEnd(n[, ch])`; поле `.length`.
Числа: `n.toFixed(digits)`.

`format`: `%d %i %f %.2f %g %s %x %X %q %%`, ширина и флаги `-` и `0`: `format("%05.1f", 3.14159)`.

**Списки:** `push(...)` (→ длина; = `append`, `add`), `pop()`, `shift()`, `unshift(...)`,
`insert(v)` / `insert(i, v)`, `remove([i])` (→ элемент), `len()`, `includes`/`contains`/`has`,
`indexOf`, `reverse()`, `join([sep = ","])`, `slice(a[, b])`, `concat(...)`, `copy()`, `clear()`,
`first()`, `last()`, `shuffle()`, `sum()`, `sort([cmp])`, `map(fn)`, `filter(fn)`, `find(fn)`,
`find_index(fn)`/`findIndex`, `each(fn)`/`forEach`, `reduce(fn[, init])`, `some(fn)`/`any`,
`every(fn)`/`all`, `count(fn)`. Колбэки получают `(item, index)`. `sort()` без компаратора — числа или
строки по возрастанию; компаратор может вернуть `true/false` («a раньше b», как в Lua) или число
(`< 0` — a раньше, как в JS); сортировка устойчивая. `table.insert`, `table.remove`,
`table.concat`, `table.sort` — для привычки к Lua.

**Словари:** `keys()`, `values()`, `entries()`, `has(k)`, `get(k[, default])`, `set(k, v)`,
`remove(k)`/`delete(k)`, `size()`/`len()`, `clear()`, `copy()`, `each(fn(key, value))`. Если в
словаре есть поле с таким именем, `m.имя()` вызывает его.

**Векторы:** `length()`/`magnitude()`, `normalized()`, `dot(v)`, `distance(v)`, `lerp(v, t)`,
`cross(v)` (vec3). **Цвета:** `lerp(c, t)`, `with_alpha(a)`.

---

## API движка

Доступно в сценариях, запущенных `ScenarioRunner`.

### Объекты

| | |
|---|---|
| `self` | объект с `ScenarioRunner` |
| `find(name)` | объект по имени или `nil` |
| `find_tag(tag)`, `find_all_tag(tag)` | первый / все (список) |
| `spawn(what, x, y[, z])`, `spawn(what, vec)` | префаб по пути (`"prefabs/enemy.prefab"`) или по имени (`"enemy"` → `prefabs/enemy.prefab`), иначе копия объекта сцены с этим именем |
| `destroy(obj[, delay])` | уничтожить (сразу или через `delay` секунд); `obj` — объект или имя |

Поля объекта (чтение и запись): `name`, `tag`, `active`, `position`, `rotation` (углы Эйлера в
градусах; запись числом — поворот вокруг Z для 2D), `scale` (запись числом — равномерно),
`parent` (объект или `nil`). Только чтение: `world_position`, `id`, `exists`. Позиция, поворот и
масштаб — локальные, `vec3`; запись `vec2` сохраняет z.

Методы объекта:

| | |
|---|---|
| `children()` | список детей |
| `find(name)` | ребёнок по имени |
| `has("Component")` | есть ли компонент |
| `get("Component.field")` | поле через reflection: `door.get("Light.intensity")` |
| `set("Component.field", value)` | запись с приведением к типу поля (`Int`, `Enum` из строки, цвет из `"#rrggbb"`) |
| `send(name[, value])` | сообщение объекту (`on_message` компонентов и сценарии на нём) |
| `destroy([delay])` | |
| `move_to(x, y[, z], seconds[, ease])`, `move_to(vec, seconds[, ease])` | плавно в локальную позицию; **сценарий ждёт** конца |
| `tween(path, value, seconds[, ease])` | анимация поля (`"Light.color"`, `"Transform.position"`, `"SpriteRenderer.color.a"`); не ждёт |

Сглаживания: `linear`, `in_quad`, `out_quad`, `in_out_quad`, `…_cubic`, `…_quart`, `…_sine`,
`…_expo`, `…_circ`, `…_back`, `…_elastic`, `…_bounce`, `smooth_step` (регистр и `_` не важны,
`"OutBack"` тоже подходит). По умолчанию — `in_out_sine`.

### Сообщения

| | |
|---|---|
| `on(name, fn(value))` | обработчик сообщений этого объекта; каждый вызов — своя задача (может ждать) |
| `wait_message(name)` | ждать сообщение, вернуть его значение |
| `emit(name[, value])` | сообщение своему объекту и всем его предкам (дойдёт в следующем кадре) |
| `broadcast(name[, value])` | всем объектам со `ScenarioRunner` + событие `ScenarioMessage` для Rust |
| `obj.send(name[, value])` | одному объекту |

### Время, ввод, звук

| | |
|---|---|
| `time()`, `dt()` | время игры и шаг кадра (с учётом `time_scale`) |
| `time_scale([value])` | прочитать / задать |
| `key(name)`, `key_down(name)` | зажата / нажата в этом кадре: `"space"`, `"a"`, `"left"`, `"enter"`, `"1"`, `"f5"`, `"shift"` |
| `axis(name)`, `action(name)`, `action_down(name)` | карты ввода (`input.bind_axis`, `bind_button`) |
| `pressed(name)`, `held(name)`, `released(name)`, `axis2(x, y)` | те же имена, что у коротких форм скриптов на Rust (`ctx.pressed("Jump")`…): действие нажато в этом кадре / зажато / отпущено; две оси сразу — `vec2` длиной не больше 1 |
| `mouse()`, `mouse_down(["left"\|"right"\|"middle"])` | позиция `vec2` в пикселях; нажатие |
| `play(clip[, volume])` | звук: имя клипа из `audio.add_clip` или путь ассета (`"sounds/hit.wav"`) |
| `music(clip[, fade = 1])`, `stop_music([fade = 1])` | музыка с плавным переходом |

### Интерфейс, сцены, сохранения

| | |
|---|---|
| `ui.text(name[, text])` | текст `Text` объекта с этим именем (прочитать / задать) |
| `ui.show(name)`, `ui.hide(name)` | включить / выключить объект интерфейса |
| `ui.value(name[, v])` | значение `Slider` |
| `ui.clicked(name)` | кнопку нажали в этом кадре (`Button.message` или имя объекта) |
| `wait_click(name)` | ждать нажатия кнопки |
| `load_scene(name)`, `reload_scene()`, `quit()` | как `ctx.load_scene` и т. д. |
| `save.get(key[, default])`, `save.set(key, value)` | `SaveData` игры; хранятся boolean, number, string, vec2, vec3; `nil` удаляет ключ |
| `log(...)`, `print(...)`, `warn(...)` | в Console, с именем объекта: `[Door] открыто` |

---

## Из Rust

```rust
use engine::prelude::*;
use engine::scenario::{self, ScriptValue, Throw};

let mut app = engine::app();                          // подсистема сценариев уже стоит
engine::mount_assets(&mut app, "assets")?;

// Своя функция для сценариев. Ошибка указывает на строку вызова в сценарии.
scenario::register(&mut app, "give_coins", |ctx, args| {
    let n = args.int(0)?;                             // bad argument #1 to `give_coins`: …
    let wallet = ctx.world.resources.get_or_insert_with(|| 0i64);
    *wallet += n;
    Ok(ScriptValue::Num(*wallet as f64))
});
scenario::register_value(&mut app, "MAX_WAVES", ScriptValue::Num(5.0));

// Запустить сценарий на объекте.
let director = app.spawn("Director").with(ScenarioRunner::new("scenarios/intro.scenario")).id();
```

* `ctx.world` — `&mut World`, `ctx.object` — объект сценария. Аргументы: `args.num(i)`,
  `args.int(i)`, `args.str(i)`, `args.boolean(i)`, `args.list(i)`, `args.map(i)`,
  `args.opt_num(i, default)`, `args.get(i)`; ошибка — `Err(Throw::msg("понятный текст"))`.
* `ScenarioRunner { script, path, auto_start = true, hot_reload = true }`. С `auto_start = false`
  сценарий ждёт `scenario::start(world, id)`; `scenario::stop(world, id)` — остановить.
* Rust → сценарий: `ctx.send_message(id, "hit", Value::Int(3))` (числа, строки, векторы, цвета,
  списки приходят как значения Scenario). Сценарий → Rust: `emit` / `obj.send` приходят в
  `on_message`; `broadcast` — ещё и событием `world.events.read::<ScenarioMessage>()`.
* Отладка и тесты: `scenario::is_running(world, id)`, `scenario::global(world, id, "имя")`
  (глобальная переменная сценария), `scenario::take_errors(world)`, `scenario::check(world, path)`,
  `scenario::limits(world)`.
* Ядро языка — крейт `engine-scenario` (`engine::scenario::lang`): `Script::compile`, `Vm`,
  `Task::resume`, `Group`. Оно не зависит от движка и работает отдельно:

```rust
use engine::scenario::lang::{Vm, Value};
let mut vm = Vm::new();
assert_eq!(vm.run_source("t.scenario", "return [3, 1, 2].sort().join()").unwrap(), Value::str("1,2,3"));
```

Сценарии читаются через базу ассетов, поэтому работают и из пака (браузер, Android): ничего
настраивать не нужно.

---

## Пределы

Зациклившийся сценарий не вешает игру и редактор. Значения по умолчанию (`scenario::limits(world)`):

| Предел | По умолчанию | Что происходит |
|---|---|---|
| `instructions_per_resume` | 200 000 | задача без `wait` продолжается в следующем кадре; одно предупреждение на сценарий |
| `max_busy_frames` | 600 | столько кадров подряд без `wait` — задача остановлена с ошибкой |
| `max_call_depth` | 200 | глубже — ошибка «too much recursion» (не падение) |
| `max_values` | 4 000 000 | элементов во всех списках и словарях |
| `max_string_len` | 4 МБ | длина одной строки |
| вложенность кода | 120 уровней | ошибка компиляции, а не переполнение стека |

Сценарий детерминирован: одинаковый код и ввод дают одинаковый результат; `random` — свой
генератор, `seed(n)` задаёт зерно. Списки и словари, ссылающиеся сами на себя, освобождаются
только с ВМ.

## Ошибки

Каждая ошибка — `файл:строка:столбец`, строка исходника с кареткой, подсказка и стек вызовов.
В Console двойной щелчок открывает файл на нужной строке. Ошибка останавливает только задачу, в
которой случилась; игра и другие сценарии работают дальше.

```
scenarios/door.scenario:12:9: tried to read `.position` of nil — `boss` is nil
 12 |   boss.position = vec3()
    |        ^
   at open_gate (scenarios/door.scenario:12:9)
   at <main> (scenarios/door.scenario:20:1)
```

| Сообщение | Что значит |
|---|---|
| ``expected `end` to close the `function` opened at line 3`` | не закрыт блок (`end` или `}`) |
| ``expected `}` to close the `{` opened at line 5`` | то же для `{` |
| ``unexpected `end` — this block was opened with `{` at line 4, close it with `}` `` | формы перепутаны |
| ``this string is never closed`` | нет закрывающей кавычки |
| `` `prnt` is not defined `` + `did you mean `print`?` | опечатка в имени (до запуска) |
| `` `hp` is not declared — write `let hp = …` `` | присваивание без объявления |
| `` `MAX` is a const and can't be changed `` | присваивание константе |
| `` `a` is already declared on line 1 `` | повторное объявление в том же блоке |
| `` `x` is used before its declaration `` | чтение до `let` |
| `this line computes a value but does nothing with it` (+ подсказка про `--` или `=`) | `i--`, `a == 1` |
| ``tried to call `f`, but it is nil`` | вызов несуществующей функции |
| ``tried to read `.hp` of nil — `player` is nil`` | доступ к полю `nil` |
| ``the map (`enemy`) has no key or method `attack` `` (+ did you mean) | нет метода |
| `can't add number and nil: the right side is nil` | арифметика с не-числом |
| ``bad argument #1 to `sqrt`: expected a number, got the string "x"`` | неверный аргумент |
| `index 5 is out of range for a list of length 1 (use push…)` | запись за концом списка |
| `too much recursion: more than 200 nested calls` | бесконечная рекурсия |
| `stopped: this task ran for 600 frames without waiting (an infinite loop?)` | цикл без `wait` |
| `out of script memory …`, `string too long …` | пределы памяти |

## Горячая перезагрузка

На десктопе и в редакторе подсистема каждые 0.25 с проверяет файлы работающих сценариев. Файл
изменился (сохранили во вкладке Code или в любом редакторе) — сценарий компилируется заново и все
его `ScenarioRunner` перезапускаются **прямо в Play**, без пересборки и перезапуска редактора;
в Console — `scenarios/x.scenario: reloaded (1 runner(s) restarted)`. Если в новой версии
ошибка, она показывается в Console, а старая версия продолжает работать до исправления.
Когда игра стоит (режим Edit), сохранённый файл тоже проверяется: синтаксическая ошибка и
неизвестные имена сразу видны в Console. Перезапуск начинает сценарий с начала: глобальные
переменные сценария не переносятся. `hot_reload = false` на раннере отключает перезапуск. В паке
(браузер, Android) файлы не меняются и не проверяются.

В редакторе: Assets → **New Scenario Script…** (а также контекстное меню папки в Project и
Add Component в инспекторе) создаёт `assets/scenarios/<имя>.scenario` из шаблона, открывает его во
вкладке Code с подсветкой и (если выбран объект) вешает `ScenarioRunner`. Двойной щелчок по
`.scenario` в Project открывает файл.

## Когда Scenario, а когда Rust / C++

| Scenario | Rust / C++ |
|---|---|
| последовательности во времени: катсцены, обучение, диалоги, отсчёты | поведение, которое работает каждый кадр: движение, физика, ИИ |
| события уровня, волны врагов, триггеры «когда A, потом B» | системы игры: инвентарь, сохранения, сеть |
| быстрые правки в Play без пересборки | всё, что требует скорости (интерпретатор в 20–50 раз медленнее Rust) |
| склейка готовых компонентов через сообщения и reflection | новые компоненты, видимые в инспекторе |

Правило: если код — это «сначала, потом, подождать, а затем», пишите сценарий; если это
«каждый кадр для каждого объекта», пишите компонент. Сценарий не объявляет компонентов и полей
инспектора: он управляет компонентами, написанными на Rust и C++.

Скорость интерпретатора (профиль dev, этот компьютер): 1 000 000 итераций цикла с арифметикой —
≈ 0.11–0.15 с; рекурсивный `fib(25)` — ≈ 0.05 с; 100 000 `push` + `sort` — ≈ 0.07 с; сортировка
100 000 чисел с компаратором-функцией — ≈ 1.3 с. Пример замера:
`cargo run -p engine-scenario --example bench`.

// FaultyScript — учебный скрипт: бросает исключение из on_update, чтобы показать, что оно
// ловится на границе (в лог идёт ошибка, игра продолжает работать). Повесьте на объект,
// включите throw_now и посмотрите консоль.

#include "engine_component.hpp"

#include <stdexcept>

struct FaultyScript : engine::Component {
    bool throw_now = false;
    std::int32_t ticks = 0;

    void on_update(engine::Object, float) override {
        ticks += 1;
        if (throw_now) throw std::runtime_error("boom");
    }
};
ENGINE_FIELDS(FaultyScript,
              ENGINE_FIELD(FaultyScript, throw_now, ENGINE_FIELD_BOOL),
              ENGINE_FIELD(FaultyScript, ticks, ENGINE_FIELD_I32));
ENGINE_COMPONENT(FaultyScript);

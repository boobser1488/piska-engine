// ApiProbe — самопроверка C++ API: по сообщению "probe.<что>" выполняет одно обращение к
// движку и записывает итог в свои поля (result, number, count, ok). Им пользуются тесты
// crates/engine/tests/cpp_api.rs и crates/engine-cpp-bridge/tests/api.rs; в играх он не нужен,
// зато это короткий справочник «как вызывается» по каждой группе API.

#include "engine_component.hpp"

#include <stdexcept>

struct ApiProbe : engine::Component {
    std::string result;
    std::string last_other;
    float number = 0.0f;
    std::int32_t count = 0;
    bool ok = false;
    // Запомненный объект (индекс и поколение) — для проверки устаревших хэндлов.
    std::int32_t stash_index = 0;
    std::int32_t stash_generation = 0;
    std::int32_t collisions = 0;
    std::int32_t triggers = 0;
    std::int32_t updates = 0;
    std::int32_t fixed_updates = 0;
    std::int32_t late_updates = 0;
    std::int32_t starts = 0;
    std::int32_t destroyed = 0;
    std::int32_t ticks = 0;
    bool throw_in_update = false;

    engine::Object stash() const {
        return engine::Object{EngineObject{static_cast<std::uint32_t>(stash_index),
                                           static_cast<std::uint32_t>(stash_generation)}};
    }
    void keep(engine::Object o) {
        stash_index = static_cast<std::int32_t>(o.id.index);
        stash_generation = static_cast<std::int32_t>(o.id.generation);
    }

    void on_start(engine::Object) override { starts += 1; }
    void on_update(engine::Object, float) override {
        updates += 1;
        if (throw_in_update) throw std::runtime_error("probe exception");
    }
    void on_fixed_update(engine::Object, float) override { fixed_updates += 1; }
    void on_late_update(engine::Object, float) override { late_updates += 1; }
    void on_destroy(engine::Object) override { destroyed += 1; }
    void on_collision_enter(engine::Object, engine::Object other) override {
        collisions += 1;
        last_other = other.name();
    }
    void on_trigger_enter(engine::Object, engine::Object other) override {
        triggers += 1;
        last_other = other.name();
    }

    void on_message(engine::Object self, const char* raw, const engine::Value& value) override {
        const std::string name = raw;
        if (name == "Tick") { ticks += 1; return; }
        if (name.rfind("probe.", 0) != 0) {
            // Любое другое сообщение просто запоминаем.
            result = name;
            number = value.to_float();
            count += 1;
            return;
        }
        const std::string what = name.substr(6);

        if (what == "spawn") {
            const engine::Object o = engine::spawn("Made-by-cpp");
            o.set_tag("made");
            o.set_position({1.0f, 2.0f, 3.0f});
            keep(o);
            ok = o.alive();
            result = o.name();
        } else if (what == "destroy_stashed") {
            stash().destroy();
            ok = stash().alive();  // до конца кадра объект ещё жив
        } else if (what == "stale") {
            // Объект уже уничтожен: чтение даёт нули, запись ничего не делает, в лог идёт ошибка.
            const engine::Object o = stash();
            ok = !o.alive();
            number = o.position().x;
            o.set_position({9.0f, 9.0f, 9.0f});
            result = o.name();
            o.destroy();
        } else if (what == "null_handle") {
            const engine::Object nothing = engine::find_by_name("no-such-object-anywhere");
            ok = !nothing && !nothing.alive();
            number = nothing.position().y;
        } else if (what == "after") {
            self.send_after(0.5f, "Tick");
        } else if (what == "every") {
            self.send_every(0.25f, "Tick");
        } else if (what == "cancel") {
            number = static_cast<float>(self.cancel_timers());
        } else if (what == "tween") {
            ok = static_cast<bool>(self.tween("ApiProbe.number", 10.0f, engine::TweenParams(1.0f, "Linear")));
        } else if (what == "fields") {
            const engine::Object t = engine::find_by_name("Label");
            ok = t.set("Text", "text", "h\xc3\xa9llo") && t.set("Text", "color", engine::Color{1.0f, 0.0f, 0.0f, 1.0f}) &&
                 t.set("Text", "align", engine::Value::enum_name("Center")) && t.set("Text", "size", 33);
            std::string text;
            t.get("Text", "text", text);
            result = text;
            t.get("Text", "size", number);
            engine::Color c;
            t.get("Text", "color", c);
            count = static_cast<std::int32_t>(c.r * 100.0f);
            std::string align;
            t.get("Text", "align", align);
            last_other = align;
        } else if (what == "bad_field") {
            const engine::Object t = engine::find_by_name("Label");
            ok = t.set("Text", "no_such_field", 1.0f);
            ok = ok || t.set("Text", "text", 5.0f);  // число в строковое поле — отказ
        } else if (what == "add_component") {
            const engine::Object o = engine::spawn("WithText");
            ok = o.add_component("Text") && o.has_component("Text") && !o.has_component("Image");
            ok = ok && o.remove_component("Text") && !o.has_component("Text");
            ok = ok && !o.add_component("NoSuchComponent");
            keep(o);
        } else if (what == "hierarchy") {
            const engine::Object parent = engine::spawn("P");
            const engine::Object child = engine::spawn("C");
            child.set_parent(parent);
            parent.set_position({10.0f, 0.0f, 0.0f});
            child.set_position({1.0f, 0.0f, 0.0f});
            number = child.world_position().x;  // 11
            count = static_cast<std::int32_t>(parent.child_count());
            ok = child.parent() == parent && parent.find_child("C") == child && parent.child(0) == child;
            child.set_world_position({20.0f, 0.0f, 0.0f});
            result = std::to_string(child.position().x);  // 10
            child.look_at({20.0f, 0.0f, -5.0f});
            last_other = std::to_string(child.forward().z);  // -1
        } else if (what == "find") {
            count = static_cast<std::int32_t>(engine::find_all_by_tag("made").size());
            ok = static_cast<bool>(engine::find_by_tag("made"));
            number = static_cast<float>(engine::find_with_component("Text").size());
        } else if (what == "exception") {
            throw std::runtime_error("boom from on_message");
        } else if (what == "time") {
            number = engine::delta_time();
            count = static_cast<std::int32_t>(engine::frame_count());
            ok = engine::time_scale() == 1.0f;
            engine::set_time_scale(0.5f);
        } else if (what == "save") {
            engine::save::set("probe.i", 5);
            engine::save::set("probe.s", "text");
            engine::save::set("probe.v", engine::Vec3{1.0f, 2.0f, 3.0f});
            count = engine::save::get_int("probe.i", -1);
            result = engine::save::get_string("probe.s");
            number = engine::save::get_int("probe.missing", 77) + engine::save::get("probe.v").to_vec3().z;
            ok = engine::save::has("probe.i") && engine::save::erase("probe.i") && !engine::save::has("probe.i");
        } else if (what == "rng") {
            engine::rng::seed(42);
            const float a = engine::rng::value();
            engine::rng::seed(42);
            ok = a == engine::rng::value() && a >= 0.0f && a < 1.0f;
            const int d = engine::rng::integer(3, 5);
            ok = ok && d >= 3 && d <= 5;
            number = static_cast<float>(d);
        } else if (what == "raycast") {
            const engine::physics::RaycastHit hit = engine::physics::raycast(value.to_vec3(), {0.0f, -1.0f, 0.0f}, 100.0f, self);
            ok = hit.hit;
            number = hit.distance;
            result = hit.object ? hit.object.name() : std::string();
            last_other = std::to_string(hit.normal.y);
        } else if (what == "raycast2d") {
            const engine::physics::RaycastHit hit = engine::physics::raycast2d(value.to_vec2(), {0.0f, -1.0f}, 100.0f);
            ok = hit.hit;
            number = hit.distance;
            result = hit.object ? hit.object.name() : std::string();
        } else if (what == "overlap") {
            count = static_cast<std::int32_t>(engine::physics::overlap_sphere(value.to_vec3(), 1.0f).size());
            number = static_cast<float>(engine::physics::overlap_circle(value.to_vec2(), 1.0f).size());
        } else if (what == "impulse") {
            const engine::Object ball = engine::find_by_name("Ball");
            engine::physics::apply_impulse(ball, {0.0f, 5.0f, 0.0f});
            number = engine::physics::mass(ball);
        } else if (what == "audio") {
            const engine::audio::Voice v = engine::audio::play("probe-beep", 0.5f, 1.5f);
            ok = static_cast<bool>(v) && v.playing();
            count = engine::audio::voice_count();
        } else if (what == "ui") {
            ok = engine::ui::set_text("Label", "Score: 7") && engine::ui::set_value("Bar", 0.25f);
            engine::ui::set_visible("Hidden", false);
            number = engine::ui::clicked("Play") ? 1.0f : 0.0f;
        } else if (what == "service_missing") {
            int status = -1;
            const auto r = engine::call_service("no.such.service", {}, 1, &status);
            ok = r.empty() && status == ENGINE_SERVICE_MISSING && !engine::service_available("no.such.service");
            count = status;
        } else if (what == "input") {
            number = engine::input::axis("Horizontal");
            ok = engine::input::key("D") && !engine::input::key("A") && engine::input::mouse_button(0);
            const engine::Vec2 m = engine::input::mouse_position();
            count = static_cast<std::int32_t>(m.x);
            result = engine::input::button_down("Jump") ? "jump" : "";
        } else if (what == "scene_name") {
            result = engine::scene_name();
        } else if (what == "load_scene") {
            engine::load_scene(value.to_string().c_str());
        } else if (what == "quit") {
            engine::quit();
        } else if (what == "destroy_self") {
            self.destroy();
            ok = self.alive();  // живёт до конца кадра
        } else {
            engine::log_warnf("ApiProbe: unknown command `%s`", what.c_str());
        }
    }
};
ENGINE_FIELDS(ApiProbe,
              ENGINE_FIELD(ApiProbe, result, ENGINE_FIELD_STRING),
              ENGINE_FIELD(ApiProbe, last_other, ENGINE_FIELD_STRING),
              ENGINE_FIELD(ApiProbe, number, ENGINE_FIELD_F32),
              ENGINE_FIELD(ApiProbe, count, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, ok, ENGINE_FIELD_BOOL),
              ENGINE_FIELD(ApiProbe, stash_index, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, stash_generation, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, collisions, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, triggers, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, updates, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, fixed_updates, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, late_updates, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, starts, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, destroyed, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, ticks, ENGINE_FIELD_I32),
              ENGINE_FIELD(ApiProbe, throw_in_update, ENGINE_FIELD_BOOL));
ENGINE_COMPONENT(ApiProbe);

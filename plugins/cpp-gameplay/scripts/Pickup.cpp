// Pickup — подбираемый предмет: игрок (тег "Player") входит в триггер — звук, очки в
// сохранении, обновление счёта на экране, предмет исчезает. Нужен Collider2D с is_trigger.

#include "engine_component.hpp"

struct Pickup : engine::Component {
    std::int32_t value = 1;
    std::string sound = "coin";
    // Имя текстового объекта интерфейса, где показывается счёт.
    std::string score_label = "ScoreText";

    void on_trigger_enter(engine::Object self, engine::Object other) override {
        if (!other.alive() || other.tag() != "Player") return;
        const int score = engine::save::get_int("score", 0) + value;
        engine::save::set("score", score);
        engine::ui::set_text(score_label.c_str(), ("Score: " + std::to_string(score)).c_str());
        if (!sound.empty()) engine::audio::play(sound.c_str());
        self.destroy();  // в конце кадра; до тех пор объект жив и безопасен
    }
};
ENGINE_FIELDS(Pickup,
              ENGINE_FIELD(Pickup, value, ENGINE_FIELD_I32),
              ENGINE_FIELD(Pickup, sound, ENGINE_FIELD_STRING),
              ENGINE_FIELD(Pickup, score_label, ENGINE_FIELD_STRING));
ENGINE_COMPONENT(Pickup);

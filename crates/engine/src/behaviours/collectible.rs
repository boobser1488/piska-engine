use engine_core::prelude::*;

component! {
    /// Подбирается при касании (монета, ключ, аптечка): объект с тегом `collector_tag`
    /// вошёл в триггер — сообщение `message` (со значением `value`) уходит подобравшему и
    /// объекту `notify` (например `Game`, который ведёт счёт), звучит `sound`, предмет исчезает.
    /// Нужен `Collider2D` / `Collider` с галочкой `is_trigger`.
    pub struct Collectible {
        /// Кто может подобрать (тег; пусто — кто угодно).
        pub collector_tag: String = String::from("Player"),
        /// Имя сообщения.
        pub message: String = String::from("Collected"),
        /// Значение сообщения (очки, жизни…).
        pub value: i32 = 1,
        /// Имя объекта, которому тоже сообщить (пусто — никому).
        pub notify: String = String::from("Game"),
        /// Звук (имя клипа или путь `sounds/coin.wav`; пусто — без звука).
        pub sound: String = String::new(),
    }
}

impl Component for Collectible {
    fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        if !self.collector_tag.is_empty() && !ctx.has_tag(other, &self.collector_tag) {
            return;
        }
        ctx.send_value(other, &self.message, self.value);
        if let Some(game) = ctx.find_by_name(&self.notify) {
            ctx.send_value(game, &self.message, self.value);
        }
        #[cfg(feature = "audio")]
        if !self.sound.is_empty() {
            use engine_audio::AudioCtx;
            ctx.play(&self.sound);
        }
        ctx.destroy_self();
    }
}

use engine_core::prelude::*;

component! {
    /// Жизни. Сообщение `Damage` (число) отнимает, `Heal` прибавляет. На нуле объект
    /// получает сообщение `Died` (его ловят другие скрипты на этом же объекте) и, если
    /// `destroy_on_death`, исчезает.
    pub struct Health {
        pub max: i32 = 3,
        pub hp: i32 = 3,
        /// Уничтожить объект, когда жизни кончились.
        pub destroy_on_death: bool = true,
    }
}

impl Health {
    /// Живой ли ещё.
    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

impl Component for Health {
    fn on_message(&mut self, ctx: &mut Ctx, message: &Message) {
        let amount = message.get::<i32>().unwrap_or(1);
        if message.is("Damage") && self.hp > 0 {
            self.hp = (self.hp - amount).max(0);
            if self.hp == 0 {
                let me = ctx.id;
                ctx.send(me, "Died");
                if self.destroy_on_death {
                    ctx.destroy_self();
                }
            }
        } else if message.is("Heal") && self.hp > 0 {
            self.hp = (self.hp + amount).min(self.max);
        }
    }
}

component! {
    /// Наносит урон тому, кого коснулся (столкновение или триггер): шлёт ему `Damage`.
    pub struct Damage {
        pub amount: i32 = 1,
        /// Бить только объекты с этим тегом (пусто — всех).
        pub only_tag: String = String::new(),
        /// Исчезнуть после удара (пуля).
        pub destroy_on_hit: bool = false,
    }
}

impl Damage {
    fn hit(&self, ctx: &mut Ctx, other: GameObjectId) {
        if !self.only_tag.is_empty() && !ctx.has_tag(other, &self.only_tag) {
            return;
        }
        ctx.send_value(other, "Damage", self.amount);
        if self.destroy_on_hit {
            ctx.destroy_self();
        }
    }
}

impl Component for Damage {
    fn on_collision(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        self.hit(ctx, other);
    }

    fn on_trigger(&mut self, ctx: &mut Ctx, other: GameObjectId) {
        self.hit(ctx, other);
    }
}

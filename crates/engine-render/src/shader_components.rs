//! Компоненты своих шейдеров (docs/api/shaders.md): эффект на весь экран [`PostEffect`],
//! шейдер спрайта [`SpriteShader`] и параметры шейдера одного объекта [`MaterialParams`].
//!
//! Нет этих компонентов в сцене — рендер их не ищет (одна проверка счётчика индекса сцены
//! за кадр) и рисует ровно как раньше.

use engine_assets::{AssetHandle, ParamValue, Shader, ShaderParams};
use engine_core::prelude::*;

component! {
    /// Эффект на весь экран своим шейдером (`//! kind: post`). Кладётся на камеру; эффектов
    /// может быть несколько — они идут по возрастанию `order`.
    pub struct PostEffect {
        /// Шейдер эффекта (`.shader` вида post).
        pub shader: AssetHandle<Shader> = AssetHandle::none(),
        /// Значения параметров шейдера (чего нет — по умолчанию из `.shader`).
        pub params: ShaderParams = ShaderParams::default(),
        /// Порядок в цепочке: меньше — раньше.
        pub order: i32 = 0,
        /// До тонмаппинга, в HDR: яркость больше 1 даёт блум. Обычно — после него.
        pub before_tonemap: bool = false,
        /// Выключенный эффект пропускается.
        pub enabled: bool = true,
    }
}

impl Component for PostEffect {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl PostEffect {
    /// Эффект с шейдером `shader`.
    pub fn new(shader: AssetHandle<Shader>) -> Self {
        Self { shader, ..Self::default() }
    }

    /// Параметр цепочкой: `PostEffect::new(s).with("strength", 0.6)`.
    pub fn with(mut self, name: &str, value: impl Into<ParamValue>) -> Self {
        self.params.set(name, value);
        self
    }

    /// Порядок в цепочке.
    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Изменить параметр (виден со следующего кадра).
    pub fn set(&mut self, name: &str, value: impl Into<ParamValue>) {
        self.params.set(name, value);
    }
}

component! {
    /// Свой шейдер спрайта (`//! kind: sprite`) для `SpriteRenderer` этого объекта. Спрайты с
    /// одним шейдером и одинаковыми параметрами по-прежнему рисуются одной пачкой.
    pub struct SpriteShader {
        /// Шейдер (`.shader` вида sprite).
        pub shader: AssetHandle<Shader> = AssetHandle::none(),
        /// Значения параметров шейдера (чего нет — по умолчанию из `.shader`).
        pub params: ShaderParams = ShaderParams::default(),
    }
}

impl Component for SpriteShader {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl SpriteShader {
    pub fn new(shader: AssetHandle<Shader>) -> Self {
        Self { shader, ..Self::default() }
    }

    pub fn with(mut self, name: &str, value: impl Into<ParamValue>) -> Self {
        self.params.set(name, value);
        self
    }

    /// Изменить параметр (виден со следующего кадра). Спрайт с особыми значениями рисуется
    /// своей пачкой.
    pub fn set(&mut self, name: &str, value: impl Into<ParamValue>) {
        self.params.set(name, value);
    }
}

component! {
    /// Параметры своего шейдера только для этого объекта (поверх значений материала).
    /// Объект с ними рисуется отдельной пачкой; остальные объекты с тем же материалом —
    /// по-прежнему вместе.
    pub struct MaterialParams {
        /// Значения параметров (имя → значение).
        pub params: ShaderParams = ShaderParams::default(),
    }
}

impl Component for MaterialParams {
    fn hooks(&self) -> Hooks {
        Hooks::NONE
    }
}

impl MaterialParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &str, value: impl Into<ParamValue>) -> Self {
        self.params.set(name, value);
        self
    }

    /// Изменить параметр (виден со следующего кадра).
    pub fn set(&mut self, name: &str, value: impl Into<ParamValue>) {
        self.params.set(name, value);
    }
}

/// Короткая запись для скриптов: `ctx.set_shader_param("amount", 0.5)` меняет параметр
/// своего шейдера у этого объекта — у `SpriteShader`, у `PostEffect` (на камере) или, для
/// мешей, в `MaterialParams` (компонент добавляется сам; объект рисуется своей пачкой,
/// общий материал не меняется).
pub trait ShaderParamExt {
    fn set_shader_param(&mut self, name: &str, value: impl Into<ParamValue>);
}

impl ShaderParamExt for Ctx<'_> {
    fn set_shader_param(&mut self, name: &str, value: impl Into<ParamValue>) {
        let object = self.object_mut();
        if let Some(sprite) = object.get_mut::<SpriteShader>() {
            sprite.set(name, value);
        } else if let Some(effect) = object.get_mut::<PostEffect>() {
            effect.set(name, value);
        } else if let Some(params) = object.get_mut::<MaterialParams>() {
            params.set(name, value);
        } else {
            object.add(MaterialParams::new().with(name, value));
        }
    }
}

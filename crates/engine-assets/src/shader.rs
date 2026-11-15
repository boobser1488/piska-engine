//! Свои шейдеры: ассет `.shader` — текст WGSL с маленьким заголовком в комментариях `//!`.
//!
//! Пользователь пишет одну функцию (`surface`, `unlit`, `sprite` или `post`), а не шейдер
//! целиком: остальное (освещение, тени, туман, тонмаппинг) собирает движок
//! (`engine_render::shaders`). Здесь — только то, что не требует GPU: разбор заголовка
//! (вид шейдера, параметры с типами, значениями по умолчанию, диапазонами и подсказками)
//! и значения параметров, которые хранят материал и компоненты.
//!
//! ```text
//! //! kind: surface
//! /// Цвет края прожига.
//! //! param edge_color: color = (1.0, 0.5, 0.1, 1.0)
//! //! param amount: float = 0.0 [0, 1]
//! //! param noise: texture = white
//! fn surface(in: SurfaceIn) -> Surface { … }
//! ```

use std::collections::BTreeMap;

use engine_core::reflect::{FieldMeta, FieldValue, Value};
use serde::{Deserialize, Serialize};

use crate::{AssetHandle, AssetType, Guid, Texture};

/// Маркер ассета `.shader` для [`AssetHandle<Shader>`].
#[derive(Debug)]
pub struct Shader;

impl AssetType for Shader {
    const IMPORTER: &'static str = "Shader";
}

/// Что делает шейдер — какую функцию пишет пользователь.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderKind {
    /// Поверхность 3D-материала: `fn surface(in: SurfaceIn) -> Surface`; свет, тени и туман —
    /// движка.
    Surface,
    /// Без освещения: `fn unlit(in: SurfaceIn) -> vec4<f32>` — цвет как есть (туман — движка).
    Unlit,
    /// Спрайт: `fn sprite(in: SpriteIn) -> vec4<f32>`.
    Sprite,
    /// Эффект на весь экран: `fn post(in: PostIn) -> vec4<f32>`.
    Post,
}

impl ShaderKind {
    pub const ALL: [ShaderKind; 4] = [ShaderKind::Surface, ShaderKind::Unlit, ShaderKind::Sprite, ShaderKind::Post];

    /// Слово в строке `//! kind: …`.
    pub fn keyword(self) -> &'static str {
        match self {
            ShaderKind::Surface => "surface",
            ShaderKind::Unlit => "unlit",
            ShaderKind::Sprite => "sprite",
            ShaderKind::Post => "post",
        }
    }

    /// Имя функции, которую пишет пользователь (совпадает со словом вида).
    pub fn function(self) -> &'static str {
        self.keyword()
    }

    /// Подпись этой функции — для сообщений об ошибках и шаблонов.
    pub fn signature(self) -> &'static str {
        match self {
            ShaderKind::Surface => "fn surface(in: SurfaceIn) -> Surface",
            ShaderKind::Unlit => "fn unlit(in: SurfaceIn) -> vec4<f32>",
            ShaderKind::Sprite => "fn sprite(in: SpriteIn) -> vec4<f32>",
            ShaderKind::Post => "fn post(in: PostIn) -> vec4<f32>",
        }
    }

    /// Для 3D-материалов (`MeshRenderer`).
    pub fn is_material(self) -> bool {
        matches!(self, ShaderKind::Surface | ShaderKind::Unlit)
    }

    fn parse(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.keyword() == word)
    }
}

/// Тип параметра шейдера.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParamType {
    Float,
    Int,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    /// Цвет RGBA (линейный); в шейдере — `vec4<f32>`.
    Color,
    /// Текстура: в шейдере `имя: texture_2d<f32>` и `имя_sampler: sampler`.
    Texture,
}

impl ParamType {
    pub const ALL: [ParamType; 8] = [
        ParamType::Float,
        ParamType::Int,
        ParamType::Bool,
        ParamType::Vec2,
        ParamType::Vec3,
        ParamType::Vec4,
        ParamType::Color,
        ParamType::Texture,
    ];

    /// Слово в объявлении `//! param имя: тип`.
    pub fn keyword(self) -> &'static str {
        match self {
            ParamType::Float => "float",
            ParamType::Int => "int",
            ParamType::Bool => "bool",
            ParamType::Vec2 => "vec2",
            ParamType::Vec3 => "vec3",
            ParamType::Vec4 => "vec4",
            ParamType::Color => "color",
            ParamType::Texture => "texture",
        }
    }

    /// Тип поля в `params` (для текстур — тип глобальной переменной).
    pub fn wgsl(self) -> &'static str {
        match self {
            ParamType::Float => "f32",
            ParamType::Int => "i32",
            ParamType::Bool => "bool",
            ParamType::Vec2 => "vec2<f32>",
            ParamType::Vec3 => "vec3<f32>",
            ParamType::Vec4 | ParamType::Color => "vec4<f32>",
            ParamType::Texture => "texture_2d<f32>",
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            "f32" => Some(ParamType::Float),
            "i32" => Some(ParamType::Int),
            "colour" => Some(ParamType::Color),
            "vec2f" => Some(ParamType::Vec2),
            "vec3f" => Some(ParamType::Vec3),
            "vec4f" => Some(ParamType::Vec4),
            _ => Self::ALL.into_iter().find(|t| t.keyword() == word),
        }
    }

    /// Значение по умолчанию, если в объявлении его нет.
    pub fn zero(self) -> ParamValue {
        match self {
            ParamType::Float => ParamValue::Float(0.0),
            ParamType::Int => ParamValue::Int(0),
            ParamType::Bool => ParamValue::Bool(false),
            ParamType::Vec2 => ParamValue::Vec2(0.0, 0.0),
            ParamType::Vec3 => ParamValue::Vec3(0.0, 0.0, 0.0),
            ParamType::Vec4 => ParamValue::Vec4(0.0, 0.0, 0.0, 0.0),
            ParamType::Color => ParamValue::Color(1.0, 1.0, 1.0, 1.0),
            ParamType::Texture => ParamValue::Texture(String::new()),
        }
    }
}

/// Что подставить вместо текстуры-параметра, которую материал не назначил.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureDefault {
    #[default]
    White,
    Black,
    Gray,
    /// Плоская карта нормалей (0.5, 0.5, 1).
    Normal,
}

impl TextureDefault {
    pub const ALL: [TextureDefault; 4] = [TextureDefault::White, TextureDefault::Black, TextureDefault::Gray, TextureDefault::Normal];

    pub fn keyword(self) -> &'static str {
        match self {
            TextureDefault::White => "white",
            TextureDefault::Black => "black",
            TextureDefault::Gray => "gray",
            TextureDefault::Normal => "normal",
        }
    }

    /// Цвет пикселя 1×1 (RGBA8).
    pub fn rgba(self) -> [u8; 4] {
        match self {
            TextureDefault::White => [255, 255, 255, 255],
            TextureDefault::Black => [0, 0, 0, 255],
            TextureDefault::Gray => [128, 128, 128, 255],
            TextureDefault::Normal => [128, 128, 255, 255],
        }
    }

    fn parse(word: &str) -> Option<Self> {
        match word {
            "grey" => Some(TextureDefault::Gray),
            _ => Self::ALL.into_iter().find(|t| t.keyword() == word),
        }
    }
}

/// Значение параметра шейдера. В файле `.material` пишется как `Float(0.5)`,
/// `Color(1.0, 0.5, 0.1, 1.0)`, `Texture("<guid>")` (пустая строка — текстура по умолчанию).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Vec2(f32, f32),
    Vec3(f32, f32, f32),
    Vec4(f32, f32, f32, f32),
    Color(f32, f32, f32, f32),
    /// GUID текстуры строкой; пусто — значение по умолчанию из объявления.
    Texture(String),
}

impl ParamValue {
    /// Тип значения.
    pub fn param_type(&self) -> ParamType {
        match self {
            ParamValue::Float(_) => ParamType::Float,
            ParamValue::Int(_) => ParamType::Int,
            ParamValue::Bool(_) => ParamType::Bool,
            ParamValue::Vec2(..) => ParamType::Vec2,
            ParamValue::Vec3(..) => ParamType::Vec3,
            ParamValue::Vec4(..) => ParamType::Vec4,
            ParamValue::Color(..) => ParamType::Color,
            ParamValue::Texture(_) => ParamType::Texture,
        }
    }

    /// Числа значения (до четырёх) — для записи в uniform-буфер.
    pub fn floats(&self) -> [f32; 4] {
        match *self {
            ParamValue::Float(x) => [x, 0.0, 0.0, 0.0],
            ParamValue::Int(i) => [i as f32, 0.0, 0.0, 0.0],
            ParamValue::Bool(b) => [if b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            ParamValue::Vec2(x, y) => [x, y, 0.0, 0.0],
            ParamValue::Vec3(x, y, z) => [x, y, z, 0.0],
            ParamValue::Vec4(x, y, z, w) | ParamValue::Color(x, y, z, w) => [x, y, z, w],
            ParamValue::Texture(_) => [0.0; 4],
        }
    }

    /// Привести значение к типу `ty` (float ↔ int, color ↔ vec4, недостающие числа — нули).
    /// `None` — не приводится (текстура и число).
    pub fn convert(&self, ty: ParamType) -> Option<ParamValue> {
        if self.param_type() == ty {
            return Some(self.clone());
        }
        if matches!(self, ParamValue::Texture(_)) || ty == ParamType::Texture {
            return None;
        }
        let [x, y, z, w] = self.floats();
        Some(match ty {
            ParamType::Float => ParamValue::Float(x),
            ParamType::Int => ParamValue::Int(x.round() as i32),
            ParamType::Bool => ParamValue::Bool(x != 0.0),
            ParamType::Vec2 => ParamValue::Vec2(x, y),
            ParamType::Vec3 => ParamValue::Vec3(x, y, z),
            ParamType::Vec4 => ParamValue::Vec4(x, y, z, w),
            ParamType::Color => ParamValue::Color(x, y, z, if matches!(self, ParamValue::Vec3(..)) { 1.0 } else { w }),
            ParamType::Texture => return None,
        })
    }

    /// Текстура значения (`None` — не текстура или не назначена).
    pub fn texture(&self) -> Option<AssetHandle<Texture>> {
        match self {
            ParamValue::Texture(text) => Guid::parse(text.trim()).map(AssetHandle::from_guid),
            _ => None,
        }
    }

    /// Значение-текстура по ссылке.
    pub fn from_texture(handle: AssetHandle<Texture>) -> Self {
        ParamValue::Texture(if handle.is_none() { String::new() } else { handle.guid().to_string() })
    }

    fn to_reflect(&self) -> Value {
        match self {
            ParamValue::Float(x) => Value::Float(*x),
            ParamValue::Int(i) => Value::Int(*i as i64),
            ParamValue::Bool(b) => Value::Bool(*b),
            ParamValue::Vec2(x, y) => Value::Vec2(*x, *y),
            ParamValue::Vec3(x, y, z) => Value::Vec3(*x, *y, *z),
            ParamValue::Vec4(x, y, z, w) => Value::Quat(*x, *y, *z, *w),
            ParamValue::Color(r, g, b, a) => Value::Color(*r, *g, *b, *a),
            ParamValue::Texture(text) => Value::Asset(text.clone()),
        }
    }

    fn from_reflect(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Float(x) => ParamValue::Float(*x),
            Value::Int(i) => ParamValue::Int(i32::try_from(*i).ok()?),
            Value::Bool(b) => ParamValue::Bool(*b),
            Value::Vec2(x, y) => ParamValue::Vec2(*x, *y),
            Value::Vec3(x, y, z) => ParamValue::Vec3(*x, *y, *z),
            Value::Quat(x, y, z, w) => ParamValue::Vec4(*x, *y, *z, *w),
            Value::Color(r, g, b, a) => ParamValue::Color(*r, *g, *b, *a),
            Value::Asset(text) => ParamValue::Texture(text.clone()),
            _ => return None,
        })
    }
}

impl From<f32> for ParamValue {
    fn from(v: f32) -> Self {
        ParamValue::Float(v)
    }
}
impl From<i32> for ParamValue {
    fn from(v: i32) -> Self {
        ParamValue::Int(v)
    }
}
impl From<bool> for ParamValue {
    fn from(v: bool) -> Self {
        ParamValue::Bool(v)
    }
}
impl From<engine_core::math::Vec2> for ParamValue {
    fn from(v: engine_core::math::Vec2) -> Self {
        ParamValue::Vec2(v.x, v.y)
    }
}
impl From<engine_core::math::Vec3> for ParamValue {
    fn from(v: engine_core::math::Vec3) -> Self {
        ParamValue::Vec3(v.x, v.y, v.z)
    }
}
impl From<engine_core::math::Vec4> for ParamValue {
    fn from(v: engine_core::math::Vec4) -> Self {
        ParamValue::Vec4(v.x, v.y, v.z, v.w)
    }
}
impl From<engine_core::math::Color> for ParamValue {
    fn from(c: engine_core::math::Color) -> Self {
        ParamValue::Color(c.r, c.g, c.b, c.a)
    }
}
impl From<AssetHandle<Texture>> for ParamValue {
    fn from(handle: AssetHandle<Texture>) -> Self {
        ParamValue::from_texture(handle)
    }
}

/// Значения параметров шейдера по именам. Чего здесь нет — берётся из объявления в
/// `.shader`. Хранится в материале (`Material::shader_params`) и в компонентах
/// (`PostEffect`, `SpriteShader`, `MaterialParams`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShaderParams(pub BTreeMap<String, ParamValue>);

impl ShaderParams {
    pub fn new() -> Self {
        Self::default()
    }

    /// Задать значение (`set("amount", 0.5)`, `set("tint", Color::RED)`).
    pub fn set(&mut self, name: &str, value: impl Into<ParamValue>) {
        self.0.insert(name.to_string(), value.into());
    }

    /// То же цепочкой: `ShaderParams::new().with("amount", 0.5)`.
    pub fn with(mut self, name: &str, value: impl Into<ParamValue>) -> Self {
        self.set(name, value);
        self
    }

    pub fn get(&self, name: &str) -> Option<&ParamValue> {
        self.0.get(name)
    }

    /// Число-параметр (float или int), если он задан.
    pub fn float(&self, name: &str) -> Option<f32> {
        match self.0.get(name)? {
            ParamValue::Float(x) => Some(*x),
            ParamValue::Int(i) => Some(*i as f32),
            _ => None,
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<ParamValue> {
        self.0.remove(name)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &ParamValue)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Текстуры, назначенные значениями.
    pub fn textures(&self) -> impl Iterator<Item = Guid> + '_ {
        self.0.values().filter_map(ParamValue::texture).map(|h| h.guid())
    }
}

/// В компоненте: список пар `[имя, значение]` — сцена хранит его как любое поле.
impl FieldValue for ShaderParams {
    const EXPECTED: &'static str = "List of [Str(name), value] pairs";

    fn to_value(&self) -> Value {
        Value::List(self.0.iter().map(|(k, v)| Value::List(vec![Value::Str(k.clone()), v.to_reflect()])).collect())
    }

    fn from_value(value: &Value) -> Option<Self> {
        let Value::List(items) = value else { return None };
        let mut map = BTreeMap::new();
        for item in items {
            let Value::List(pair) = item else { return None };
            let [Value::Str(name), value] = pair.as_slice() else { return None };
            map.insert(name.clone(), ParamValue::from_reflect(value)?);
        }
        Some(ShaderParams(map))
    }

    fn meta() -> FieldMeta {
        // Инспектор узнаёт поле параметров шейдера по этому «виду ассета».
        FieldMeta { variants: &[], asset_kind: SHADER_PARAMS_KIND }
    }
}

/// `FieldMeta::asset_kind` поля [`ShaderParams`]: инспектор рисует для него виджеты
/// параметров шейдера соседнего поля `shader`.
pub const SHADER_PARAMS_KIND: &str = "ShaderParams";

/// Объявление параметра из строки `//! param …`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub ty: ParamType,
    /// Значение по умолчанию (у текстуры — пустая строка: см. `texture_default`).
    pub default: ParamValue,
    /// Диапазон `[min, max]` для float / int — ползунок в инспекторе.
    pub range: Option<(f32, f32)>,
    /// Чем заменить неназначенную текстуру.
    pub texture_default: TextureDefault,
    /// Подсказка из комментариев `///` над объявлением.
    pub doc: String,
    /// Строка объявления (с единицы).
    pub line: usize,
}

/// Заголовок шейдера: вид и параметры.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderInfo {
    pub kind: ShaderKind,
    pub params: Vec<ParamDecl>,
}

impl ShaderInfo {
    pub fn param(&self, name: &str) -> Option<&ParamDecl> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Значение параметра с учётом слоёв значений (`layers` — от общего к частному:
    /// материал, затем объект). Неподходящий тип приводится; не приводится — по умолчанию.
    pub fn value(&self, decl: &ParamDecl, layers: &[&ShaderParams]) -> ParamValue {
        let mut value = decl.default.clone();
        for layer in layers {
            if let Some(v) = layer.get(&decl.name).and_then(|v| v.convert(decl.ty)) {
                value = v;
            }
        }
        value
    }

    /// Текстуры-параметры по порядку объявления.
    pub fn textures(&self) -> impl Iterator<Item = &ParamDecl> {
        self.params.iter().filter(|p| p.ty == ParamType::Texture)
    }
}

/// Ошибка в заголовке `.shader`: место (строка и столбец с единицы) и что делать.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ShaderHeaderError {
    pub line: usize,
    pub column: usize,
    /// Длина подчёркиваемого фрагмента (в символах; 0 — до конца строки).
    pub len: usize,
    pub message: String,
}

/// Имена, которые нельзя дать параметру: заняты движком или словами WGSL.
const RESERVED: &[&str] = &[
    "params", "in", "out", "globals", "material", "frame", "scene", "fn", "let", "var", "const", "if", "else",
    "for", "loop", "return", "struct", "true", "false", "discard", "switch", "while", "break", "continue", "f32", "i32",
    "u32", "bool", "vec2", "vec3", "vec4", "mat2x2", "mat3x3", "mat4x4", "texture", "sampler", "override", "alias",
    // Зарезервированные слова WGSL, которые легко взять именем.
    "from", "target", "type", "set", "match", "filter", "pass", "new", "self", "mod", "use", "move", "ref", "where",
    "with", "static", "enum", "layout", "shared", "mut", "smooth", "precision", "common", "attribute", "varying",
    "enable", "default", "case", "continuing", "diagnostic", "requires", "const_assert", "array", "atomic", "ptr",
];

/// Разобрать заголовок шейдера (строки `//! kind:` и `//! param`).
pub fn parse_header(text: &str) -> Result<ShaderInfo, ShaderHeaderError> {
    let mut kind: Option<ShaderKind> = None;
    let mut params: Vec<ParamDecl> = Vec::new();
    let mut doc: Vec<String> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim_start();
        let indent = raw.len() - trimmed.len();
        if let Some(rest) = trimmed.strip_prefix("///") {
            if !rest.starts_with('/') {
                doc.push(rest.trim().to_string());
                continue;
            }
        }
        let Some(rest) = trimmed.strip_prefix("//!") else {
            if !trimmed.is_empty() {
                doc.clear();
            }
            continue;
        };
        let body_column = indent + 4 + (rest.len() - rest.trim_start().len());
        let body = rest.trim();
        let err = |column: usize, len: usize, message: String| ShaderHeaderError { line, column, len, message };
        if let Some(value) = body.strip_prefix("kind:") {
            let word = value.trim();
            let column = body_column + 5 + (value.len() - value.trim_start().len());
            match ShaderKind::parse(word) {
                Some(k) if kind.is_none() => kind = Some(k),
                Some(_) => return Err(err(body_column, body.chars().count(), "the shader kind is given twice; keep one `//! kind:` line".into())),
                None => {
                    return Err(err(
                        column,
                        word.chars().count(),
                        format!("unknown shader kind `{word}`: use surface, unlit, sprite or post"),
                    ))
                }
            }
        } else if let Some(decl) = body.strip_prefix("param") {
            if !decl.starts_with(char::is_whitespace) {
                return Err(err(body_column, 5, "write `//! param name: type = value`".into()));
            }
            let decl_column = body_column + 5 + (decl.len() - decl.trim_start().len());
            let mut param = parse_param(decl.trim(), line, decl_column)?;
            param.doc = doc.join(" ").trim().to_string();
            if params.iter().any(|p| p.name == param.name) {
                return Err(err(decl_column, param.name.len(), format!("parameter `{}` is declared twice", param.name)));
            }
            params.push(param);
        } else if !body.is_empty() {
            let word: String = body.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let mut message = format!("unknown header line `//! {body}`: the header knows `kind:` and `param`");
            if let Some(best) = closest(&word, &["kind", "param"]) {
                message = format!("unknown header line `//! {body}`; did you mean `{best}`?");
            }
            return Err(err(body_column, body.chars().count(), message));
        }
        doc.clear();
    }
    let kind = kind.ok_or(ShaderHeaderError {
        line: 1,
        column: 1,
        len: 0,
        message: "the shader has no kind: add `//! kind: surface` (or unlit, sprite, post) as the first line".into(),
    })?;
    Ok(ShaderInfo { kind, params })
}

fn parse_param(decl: &str, line: usize, column: usize) -> Result<ParamDecl, ShaderHeaderError> {
    let err = |offset: usize, len: usize, message: String| ShaderHeaderError { line, column: column + offset, len, message };
    let Some((name_part, rest)) = decl.split_once(':') else {
        return Err(err(0, decl.chars().count(), "a parameter needs a type: `//! param amount: float = 0.5`".into()));
    };
    let name = name_part.trim();
    let valid = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with("__");
    if !valid {
        return Err(err(0, name.chars().count().max(1), format!("`{name}` is not a valid parameter name (letters, digits and _)")));
    }
    if RESERVED.contains(&name) || name.ends_with("_sampler") {
        return Err(err(0, name.len(), format!("`{name}` is reserved; choose another parameter name")));
    }
    let rest_offset = name_part.len() + 1;
    // Тип, затем «= значение», затем «[min, max]».
    let (type_part, after_type) = match rest.find(['=', '[']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let type_word = type_part.trim();
    let type_offset = rest_offset + (type_part.len() - type_part.trim_start().len());
    let ty = ParamType::parse(type_word).ok_or_else(|| {
        let names: Vec<&str> = ParamType::ALL.iter().map(|t| t.keyword()).collect();
        let mut message = format!("unknown parameter type `{type_word}`: use {}", names.join(", "));
        if let Some(best) = closest(type_word, &names) {
            message = format!("unknown parameter type `{type_word}`; did you mean `{best}`?");
        }
        err(type_offset, type_word.chars().count().max(1), message)
    })?;
    let mut default = ty.zero();
    let mut texture_default = TextureDefault::White;
    let mut range = None;
    let mut tail = after_type;
    let mut tail_offset = rest_offset + type_part.len();
    if let Some(value_text) = tail.strip_prefix('=') {
        // Значение — до диапазона `[` (у векторов значение в круглых скобках).
        let end = value_text.find('[').unwrap_or(value_text.len());
        let raw = &value_text[..end];
        let value_offset = tail_offset + 1 + (raw.len() - raw.trim_start().len());
        let value = raw.trim();
        if ty == ParamType::Texture {
            texture_default = TextureDefault::parse(value).ok_or_else(|| {
                err(value_offset, value.chars().count().max(1), format!("unknown texture default `{value}`: use white, black, gray or normal"))
            })?;
        } else {
            default = parse_value(ty, value).ok_or_else(|| {
                err(value_offset, value.chars().count().max(1), format!("`{value}` is not a {} value; {}", ty.keyword(), example(ty)))
            })?;
        }
        tail_offset += 1 + end;
        tail = &value_text[end..];
    }
    let tail_trimmed = tail.trim();
    if !tail_trimmed.is_empty() {
        let range_offset = tail_offset + (tail.len() - tail.trim_start().len());
        let inner = tail_trimmed.strip_prefix('[').and_then(|t| t.strip_suffix(']'));
        let parsed = inner.and_then(|inner| {
            let (a, b) = inner.split_once(',')?;
            Some((a.trim().parse::<f32>().ok()?, b.trim().parse::<f32>().ok()?))
        });
        match (ty, parsed) {
            (ParamType::Float | ParamType::Int, Some((min, max))) if min < max => range = Some((min, max)),
            (ParamType::Float | ParamType::Int, _) => {
                return Err(err(range_offset, tail_trimmed.chars().count(), "a range is `[min, max]` with min < max, e.g. `[0, 1]`".into()))
            }
            _ => return Err(err(range_offset, tail_trimmed.chars().count(), format!("a {} parameter has no range", ty.keyword()))),
        }
    }
    Ok(ParamDecl { name: name.to_string(), ty, default, range, texture_default, doc: String::new(), line })
}

fn example(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Float => "write a number like `0.5`",
        ParamType::Int => "write a whole number like `3`",
        ParamType::Bool => "write `true` or `false`",
        ParamType::Vec2 => "write `(x, y)`",
        ParamType::Vec3 => "write `(x, y, z)`",
        ParamType::Vec4 => "write `(x, y, z, w)`",
        ParamType::Color => "write `(r, g, b, a)` or `(r, g, b)` with numbers 0..1",
        ParamType::Texture => "write white, black, gray or normal",
    }
}

/// Значение из текста объявления: `0.5`, `3`, `true`, `(1.0, 0.5, 0.1)`.
fn parse_value(ty: ParamType, text: &str) -> Option<ParamValue> {
    let numbers = |count: usize| -> Option<Vec<f32>> {
        let inner = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')).unwrap_or(text);
        let list: Option<Vec<f32>> = inner.split(',').map(|n| n.trim().parse::<f32>().ok()).collect();
        let list = list?;
        (list.len() == count || (ty == ParamType::Color && list.len() == 3)).then_some(list)
    };
    Some(match ty {
        ParamType::Float => ParamValue::Float(text.parse().ok()?),
        ParamType::Int => ParamValue::Int(text.parse().ok()?),
        ParamType::Bool => ParamValue::Bool(match text {
            "true" => true,
            "false" => false,
            _ => return None,
        }),
        ParamType::Vec2 => {
            let v = numbers(2)?;
            ParamValue::Vec2(v[0], v[1])
        }
        ParamType::Vec3 => {
            let v = numbers(3)?;
            ParamValue::Vec3(v[0], v[1], v[2])
        }
        ParamType::Vec4 => {
            let v = numbers(4)?;
            ParamValue::Vec4(v[0], v[1], v[2], v[3])
        }
        ParamType::Color => {
            let v = numbers(4)?;
            ParamValue::Color(v[0], v[1], v[2], v.get(3).copied().unwrap_or(1.0))
        }
        ParamType::Texture => return None,
    })
}

/// Ближайшее по написанию слово (расстояние Левенштейна не больше трети длины, минимум 2).
pub fn closest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let limit = (word.chars().count() / 3).max(2);
    candidates
        .iter()
        .map(|c| (levenshtein(word, c), *c))
        .filter(|(d, c)| *d > 0 && *d <= limit && !c.is_empty())
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut previous = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let current = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(previous + usize::from(a[i - 1] != b[j - 1]));
            previous = current;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISSOLVE: &str = "//! kind: surface\n/// Edge glow colour.\n//! param edge_color: color = (1.0, 0.5, 0.1, 1.0)\n//! param amount: float = 0.0 [0, 1]\n//! param noise: texture = white\n//! param steps: int = 3 [1, 8]\n//! param flip: bool = true\n//! param wind: vec3 = (1, 0, 0.5)\nfn surface(in: SurfaceIn) -> Surface { return default_surface(in); }\n";

    #[test]
    fn header_declares_kind_and_typed_params() {
        let info = parse_header(DISSOLVE).unwrap();
        assert_eq!(info.kind, ShaderKind::Surface);
        let names: Vec<_> = info.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["edge_color", "amount", "noise", "steps", "flip", "wind"]);
        assert_eq!(info.params[0].default, ParamValue::Color(1.0, 0.5, 0.1, 1.0));
        assert_eq!(info.params[0].doc, "Edge glow colour.");
        assert_eq!(info.params[1].range, Some((0.0, 1.0)));
        assert_eq!(info.params[2].ty, ParamType::Texture);
        assert_eq!(info.params[3].default, ParamValue::Int(3));
        assert_eq!(info.params[4].default, ParamValue::Bool(true));
        assert_eq!(info.params[5].default, ParamValue::Vec3(1.0, 0.0, 0.5));
        assert_eq!(info.params[1].line, 4);
        assert!(info.params[1].doc.is_empty(), "docs attach to the next param only");
    }

    #[test]
    fn header_errors_point_at_the_problem() {
        let e = parse_header("fn surface() {}").unwrap_err();
        assert!(e.message.contains("//! kind: surface"), "{e}");
        let e = parse_header("//! kind: surfce\n").unwrap_err();
        assert_eq!((e.line, e.column), (1, 11));
        assert!(e.message.contains("surfce"));
        let text = "//! kind: post\n//! param amount: flaot = 1\n";
        let e = parse_header(text).unwrap_err();
        assert_eq!(e.line, 2);
        assert!(e.message.contains("did you mean `float`"), "{e}");
        let line = text.lines().nth(1).unwrap();
        assert_eq!(&line[e.column - 1..e.column - 1 + e.len], "flaot");
        let e = parse_header("//! kind: post\n//! param a: float = x\n").unwrap_err();
        assert!(e.message.contains("not a float"), "{e}");
        let e = parse_header("//! kind: post\n//! param a: float = 1 [2, 1]\n").unwrap_err();
        assert!(e.message.contains("min < max"), "{e}");
        let e = parse_header("//! kind: post\n//! param params: float\n").unwrap_err();
        assert!(e.message.contains("reserved"), "{e}");
        let e = parse_header("//! kind: post\n//! parm a: float\n").unwrap_err();
        assert!(e.message.contains("did you mean `param`"), "{e}");
    }

    #[test]
    fn values_convert_and_layer() {
        let info = parse_header(DISSOLVE).unwrap();
        let material = ShaderParams::new().with("amount", 0.25).with("steps", 5.0);
        let object = ShaderParams::new().with("amount", 0.75);
        let amount = info.param("amount").unwrap();
        assert_eq!(info.value(amount, &[&material]), ParamValue::Float(0.25));
        assert_eq!(info.value(amount, &[&material, &object]), ParamValue::Float(0.75));
        assert_eq!(info.value(info.param("steps").unwrap(), &[&material]), ParamValue::Int(5));
        assert_eq!(info.value(info.param("edge_color").unwrap(), &[]), ParamValue::Color(1.0, 0.5, 0.1, 1.0));
    }

    #[test]
    fn params_survive_ron_and_reflection() {
        let params = ShaderParams::new()
            .with("amount", 0.5)
            .with("tint", engine_core::math::Color::rgb(1.0, 0.0, 0.5))
            .with("noise", AssetHandle::<Texture>::from_guid(Guid(0xabc)));
        let text = ron::to_string(&params).unwrap();
        assert!(text.contains("\"amount\":Float(0.5)"), "{text}");
        assert_eq!(ron::from_str::<ShaderParams>(&text).unwrap(), params);
        let value = params.to_value();
        assert_eq!(ShaderParams::from_value(&value), Some(params.clone()));
        assert_eq!(params.textures().collect::<Vec<_>>(), vec![Guid(0xabc)]);
    }
}

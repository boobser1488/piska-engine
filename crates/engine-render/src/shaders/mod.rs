//! Свои шейдеры (`.shader`, docs/api/shaders.md): сборка шейдера из функции пользователя и
//! частей движка, проверка naga и перевод ошибок на строки пользователя.
//!
//! Одна функция сборки ([`assemble`]) и одна проверка ([`compile`], [`check`]) — и для
//! рендерера, и для вкладки Code редактора: они не могут разойтись во мнении, верен ли
//! шейдер.
//!
//! Сборка: части движка (для мешей — common.wgsl, вариант теней и ламп, как у стандартного
//! шейдера, renderer/variant.rs), встроенные функции (helpers.wgsli), объявления параметров
//! (структура `Params`, uniform-блок с выравниванием WGSL, текстуры и сэмплеры), переходник
//! к функции пользователя — и в конце сам текст `.shader` без изменений. Поэтому место
//! ошибки в коде пользователя — просто сдвиг.

use std::fmt;
use std::ops::Range;

use engine_assets::{closest, parse_header, ParamDecl, ParamType, ParamValue, ShaderInfo, ShaderKind, ShaderParams};

const COMMON: &str = include_str!("../common.wgsl");
const SHADOW_SINGLE: &str = include_str!("../shadow_single.wgsl");
const SHADOW_CASCADES: &str = include_str!("../shadow.wgsl");
const LIGHTS_ON: &str = include_str!("../lights.wgsl");
const LIGHTS_OFF: &str = include_str!("../lights_off.wgsl");
const HELPERS: &str = include_str!("helpers.wgsli");
const SURFACE: &str = include_str!("surface.wgsli");
const SPRITE: &str = include_str!("sprite.wgsli");
const POST: &str = include_str!("post.wgsli");

/// Бит варианта шейдера мешей: каскады теней (как `renderer/variant.rs`).
pub(crate) const VARIANT_CASCADES: usize = 1;
/// Бит варианта шейдера мешей: лампы (lights.wgsl).
pub(crate) const VARIANT_LIGHTS: usize = 2;
/// Сколько вариантов у шейдеров мешей.
pub(crate) const VARIANT_COUNT: usize = 4;
/// Вариант для прохода теней: вместо карты теней — заглушка (в проходе теней карта —
/// цель, а не текстура), без ламп. Группа 0 — только параметры кадра.
pub(crate) const SHADOW_PASS: usize = VARIANT_COUNT;

const SHADOW_STUB: &str = "// Проход теней: тени не читаются.\nfn sun_shadow(world_position: vec3<f32>, n: vec3<f32>) -> f32 {\n    return 1.0;\n}\n";

/// Сколько текстур-параметров можно объявить (лимиты WebGL2: 16 текстур на стадию, из них
/// до десяти заняты тенями, лампами и стандартным материалом).
pub const MAX_TEXTURE_PARAMS: usize = 4;

/// Номер группы привязок своего шейдера: время кадра (0), параметры (1), текстуры (2…).
pub(crate) fn custom_group(kind: ShaderKind) -> u32 {
    match kind {
        ShaderKind::Post => 1,
        _ => 2,
    }
}

/// Ошибка шейдера на месте в тексте пользователя.
#[derive(Clone, Debug, PartialEq)]
pub struct ShaderDiagnostic {
    /// Строка и столбец (с единицы; столбец — в символах).
    pub line: usize,
    pub column: usize,
    /// Подчёркиваемые байты текста `.shader`.
    pub range: Range<usize>,
    pub message: String,
    /// Пояснения: «did you mean …», нужная подпись функции, исходное сообщение naga.
    pub notes: Vec<String>,
}

impl fmt::Display for ShaderDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)?;
        for note in &self.notes {
            write!(f, " ({note})")?;
        }
        Ok(())
    }
}

/// Раскладка uniform-блока параметров: смещение каждого числового параметра (по правилам
/// выравнивания WGSL для `var<uniform>`) и размер блока.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamLayout {
    /// (номер параметра в `ShaderInfo::params`, смещение в байтах).
    pub fields: Vec<(usize, u32)>,
    /// Номера параметров-текстур по порядку привязок.
    pub textures: Vec<usize>,
    /// Размер блока (кратен 16, не меньше 16).
    pub size: u32,
}

impl ParamLayout {
    pub fn of(info: &ShaderInfo) -> Self {
        let mut fields = Vec::new();
        let mut textures = Vec::new();
        let mut offset = 0u32;
        for (index, param) in info.params.iter().enumerate() {
            let (size, align) = match param.ty {
                ParamType::Float | ParamType::Int | ParamType::Bool => (4, 4),
                ParamType::Vec2 => (8, 8),
                ParamType::Vec3 => (12, 16),
                ParamType::Vec4 | ParamType::Color => (16, 16),
                ParamType::Texture => {
                    textures.push(index);
                    continue;
                }
            };
            offset = offset.div_ceil(align) * align;
            fields.push((index, offset));
            offset += size;
        }
        let size = (offset.div_ceil(16) * 16).max(16);
        Self { fields, textures, size }
    }

    /// Байты uniform-блока для значений `layers` (от общего к частному: материал, объект).
    pub fn bytes(&self, info: &ShaderInfo, layers: &[&ShaderParams]) -> Vec<u8> {
        let mut out = vec![0u8; self.size as usize];
        for &(index, offset) in &self.fields {
            let decl = &info.params[index];
            let value = info.value(decl, layers);
            let offset = offset as usize;
            match value {
                ParamValue::Int(i) => out[offset..offset + 4].copy_from_slice(&i.to_le_bytes()),
                ParamValue::Bool(b) => out[offset..offset + 4].copy_from_slice(&u32::from(b).to_le_bytes()),
                other => {
                    let count = match decl.ty {
                        ParamType::Vec2 => 2,
                        ParamType::Vec3 => 3,
                        ParamType::Vec4 | ParamType::Color => 4,
                        _ => 1,
                    };
                    for (k, v) in other.floats().iter().take(count).enumerate() {
                        out[offset + k * 4..offset + k * 4 + 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
            }
        }
        out
    }
}

/// Собранный шейдер: исходник WGSL и где в нём текст пользователя.
#[derive(Clone, Debug)]
pub struct Assembly {
    pub source: String,
    /// Байт, с которого в `source` начинается текст `.shader`.
    pub user_start: usize,
}

/// Текст без комментариев (комментарии заменены пробелами — смещения те же).
fn strip_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    let mut depth = 0usize;
    while i < bytes.len() {
        if depth == 0 && bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            depth += 1;
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            continue;
        }
        if depth > 0 && bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
            depth -= 1;
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            continue;
        }
        if depth > 0 && bytes[i] != b'\n' {
            out[i] = b' ';
        }
        i += 1;
    }
    // Заменялись только ASCII-байты и байты внутри комментариев: UTF-8 мог порваться
    // только внутри комментария, а там теперь пробелы.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Начало объявления `fn name` в тексте без комментариев.
fn find_fn(code: &str, name: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(pos) = code[from..].find("fn") {
        let at = from + pos;
        from = at + 2;
        if at > 0 && is_ident(bytes[at - 1]) {
            continue;
        }
        let rest = &code[at + 2..];
        let trimmed = rest.trim_start();
        if trimmed.len() == rest.len() {
            continue;
        }
        if trimmed.starts_with(name) && !trimmed.as_bytes().get(name.len()).is_some_and(|&b| is_ident(b)) {
            return Some(at);
        }
    }
    None
}

/// Есть ли в тексте без комментариев слово `word` целиком.
fn has_word(code: &str, word: &str) -> bool {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(pos) = code[from..].find(word) {
        let at = from + pos;
        from = at + word.len();
        let before = at > 0 && is_ident(bytes[at - 1]);
        let after = bytes.get(at + word.len()).is_some_and(|&b| is_ident(b));
        if !before && !after {
            return true;
        }
    }
    false
}

/// Объявления параметров: структура `params`, uniform-блок, текстуры, загрузка в начале
/// каждой точки входа (`engine_load_params`).
fn params_source(info: &ShaderInfo, group: u32) -> String {
    let layout = ParamLayout::of(info);
    let mut user = String::from("struct Params {\n");
    let mut raw = String::from("struct EngineParamsRaw {\n");
    let mut load = Vec::new();
    for &(index, _) in &layout.fields {
        let p = &info.params[index];
        user.push_str(&format!("    {}: {},\n", p.name, p.ty.wgsl()));
        let raw_ty = if p.ty == ParamType::Bool { "u32" } else { p.ty.wgsl() };
        raw.push_str(&format!("    {}: {},\n", p.name, raw_ty));
        load.push(if p.ty == ParamType::Bool {
            format!("engine_params.{} != 0u", p.name)
        } else {
            format!("engine_params.{}", p.name)
        });
    }
    if layout.fields.is_empty() {
        user.push_str("    unused: f32,\n");
        raw.push_str("    unused: vec4<f32>,\n");
        load.push("0.0".to_string());
    }
    user.push_str("};\n");
    raw.push_str("};\n");
    let mut out = String::new();
    out.push_str("// ---- время кадра и параметры (shaders/mod.rs) ----\n");
    out.push_str("struct EngineFrame {\n    time: f32,\n    delta_time: f32,\n    unused0: f32,\n    unused1: f32,\n    screen: vec4<f32>,\n};\n");
    out.push_str(&format!("@group({group}) @binding(0) var<uniform> frame: EngineFrame;\n"));
    out.push_str(&user);
    out.push_str(&raw);
    out.push_str(&format!("@group({group}) @binding(1) var<uniform> engine_params: EngineParamsRaw;\n"));
    out.push_str("var<private> params: Params;\n");
    out.push_str(&format!("fn engine_load_params() {{\n    params = Params({});\n}}\n", load.join(", ")));
    for (k, &index) in layout.textures.iter().enumerate() {
        let name = &info.params[index].name;
        let binding = 2 + 2 * k as u32;
        out.push_str(&format!("@group({group}) @binding({binding}) var {name}: texture_2d<f32>;\n"));
        out.push_str(&format!("@group({group}) @binding({}) var {name}_sampler: sampler;\n", binding + 1));
    }
    out
}

/// Собрать шейдер вида `info.kind` из текста `.shader`. `variant` — вариант теней и ламп
/// (только для `surface` / `unlit`, иначе не важен).
pub fn assemble(text: &str, info: &ShaderInfo, variant: usize) -> Assembly {
    let code = strip_comments(text);
    let mut engine = String::new();
    match info.kind {
        ShaderKind::Surface | ShaderKind::Unlit => {
            engine.push_str(COMMON);
            engine.push('\n');
            let shadow_pass = variant == SHADOW_PASS;
            engine.push_str(if shadow_pass {
                SHADOW_STUB
            } else if variant & VARIANT_CASCADES != 0 {
                SHADOW_CASCADES
            } else {
                SHADOW_SINGLE
            });
            engine.push('\n');
            engine.push_str(if !shadow_pass && variant & VARIANT_LIGHTS != 0 { LIGHTS_ON } else { LIGHTS_OFF });
            engine.push('\n');
            engine.push_str(HELPERS);
            engine.push('\n');
            engine.push_str(SURFACE);
            engine.push('\n');
            let object_space = has_word(&code, "object_position");
            engine.push_str(&format!("const ENGINE_OBJECT_SPACE: bool = {object_space};\n"));
            if info.kind == ShaderKind::Surface {
                engine.push_str("const ENGINE_UNLIT: bool = false;\n");
                engine.push_str("fn engine_custom_surface(in: SurfaceIn) -> Surface {\n    return surface(in);\n}\n");
            } else {
                engine.push_str("const ENGINE_UNLIT: bool = true;\n");
                engine.push_str(
                    "fn engine_custom_surface(in: SurfaceIn) -> Surface {\n    let c = unlit(in);\n    var s = default_surface(in);\n    s.albedo = c.rgb;\n    s.alpha = c.a;\n    s.emissive = vec3<f32>(0.0);\n    return s;\n}\n",
                );
            }
            if find_fn(&code, "vertex").is_none() {
                engine.push_str("fn vertex(in: VertexIn) -> VertexOut {\n    return default_vertex(in);\n}\n");
            }
        }
        ShaderKind::Sprite => {
            engine.push_str(HELPERS);
            engine.push('\n');
            engine.push_str(SPRITE);
            engine.push('\n');
        }
        ShaderKind::Post => {
            engine.push_str(HELPERS);
            engine.push('\n');
            engine.push_str(POST);
            engine.push('\n');
        }
    }
    engine.push_str(&params_source(info, custom_group(info.kind)));
    engine.push_str("// ---- текст .shader ----\n");
    let user_start = engine.len();
    engine.push_str(text);
    engine.push('\n');
    Assembly { source: engine, user_start }
}

/// Проверенный шейдер, готовый для GPU.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub info: ShaderInfo,
    pub layout: ParamLayout,
    /// Исходник каждого варианта (для `surface` / `unlit` — [`VARIANT_COUNT`] штук и
    /// проход теней [`SHADOW_PASS`], иначе один).
    pub sources: Vec<String>,
    /// Есть своя функция `vertex` (сдвиг вершин — нужен свой проход теней).
    pub has_vertex: bool,
    /// В тексте есть `discard` (вырез — нужен свой проход теней).
    pub has_discard: bool,
}

/// Встроенные функции и переменные: имя текстуры-параметра не может с ними совпасть.
const BUILTIN_GLOBALS: &[&str] = &[
    "hash", "hash3", "value_noise", "fbm", "remap", "fresnel", "rotate_uv", "luminance", "rgb_to_hsv", "hsv_to_rgb",
    "default_surface", "default_vertex", "default_sprite", "sample_sprite", "scene", "sun_direction", "sun_color",
    "sun_visibility", "sky_ambient", "base_texture", "metallic_roughness_texture", "normal_texture", "emissive_texture",
    "occlusion_texture", "material_sampler", "sprite_texture", "sprite_sampler", "scene_texture", "scene_sampler",
    "globals", "surface", "unlit", "sprite", "post", "vertex", "shadow_map", "shadow_sampler",
];

/// Где в тексте пользователя определена его главная функция (для ошибок вне его кода).
fn main_fn_range(text: &str, kind: ShaderKind) -> Range<usize> {
    let code = strip_comments(text);
    match find_fn(&code, kind.function()) {
        Some(at) => {
            let end = text[at..].find(['{', '\n']).map_or(text.len(), |e| at + e);
            at..end.max(at + 2)
        }
        None => 0..text.lines().next().map_or(0, str::len),
    }
}

/// Строка и столбец (с единицы) байта `offset`.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

fn diagnostic(text: &str, range: Range<usize>, message: String, notes: Vec<String>) -> ShaderDiagnostic {
    let (line, column) = line_column(text, range.start);
    ShaderDiagnostic { line, column, range, message, notes }
}

/// Поля входов и выходов, имена параметров и встроенные функции — для «did you mean».
fn known_names(info: &ShaderInfo) -> Vec<&str> {
    let mut names: Vec<&str> = vec![
        // SurfaceIn / Surface / VertexIn / VertexOut
        "uv", "world_position", "world_normal", "view_dir", "color", "time", "screen_uv", "object_position", "front_facing",
        "albedo", "alpha", "normal", "metallic", "roughness", "emissive", "occlusion", "position", "object_origin",
        // SpriteIn / PostIn
        "texel_size", "texture_size", "frame_rect", "texture_color", "resolution",
        // встроенные функции и переменные
        "params", "default_surface", "default_vertex", "default_sprite", "sample_sprite", "scene", "hash", "hash3", "value_noise",
        "fbm", "remap", "fresnel", "rotate_uv", "luminance", "rgb_to_hsv", "hsv_to_rgb", "PI", "TAU", "base_texture",
        "material_sampler", "sprite_texture", "sprite_sampler", "sun_direction", "sun_color", "sun_visibility", "sky_ambient",
    ];
    for p in &info.params {
        names.push(&p.name);
    }
    names
}

/// Перевести ошибку naga (место в собранном исходнике) на текст пользователя.
fn map_error(
    text: &str,
    info: &ShaderInfo,
    assembly: &Assembly,
    span: Option<Range<usize>>,
    message: String,
    labels: Vec<(Range<usize>, String)>,
) -> ShaderDiagnostic {
    let user = assembly.user_start..assembly.user_start + text.len();
    let mut notes = Vec::new();
    // Слово в кавычках из сообщения naga: `uvv` или 'paramz'.
    let quoted = message
        .split(['`', '\''])
        .nth(1)
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(str::to_string);
    if let Some(word) = &quoted {
        if message.contains("field accessor") || message.contains("no definition in scope") {
            if let Some(best) = closest(word, &known_names(info)) {
                notes.push(format!("did you mean `{best}`?"));
            }
        }
    }
    let in_user = span.as_ref().filter(|r| r.start >= user.start && r.end <= user.end + 1);
    match in_user {
        Some(range) => {
            for (label_range, label) in &labels {
                if !label.is_empty() && *label != message && label_range.start >= user.start {
                    let (line, _) = line_column(text, label_range.start - user.start);
                    notes.push(format!("line {line}: {label}"));
                }
            }
            let mut start = range.start - user.start;
            let mut end = (range.end - user.start).min(text.len());
            // «Нет `;`»: naga ставит ошибку на следующий токен (часто на строку ниже) —
            // переносим её в конец предыдущего, где точка с запятой и пропущена.
            if message.starts_with("expected ';'") {
                let before = text[..start].trim_end().len();
                if let Some((i, c)) = text[..before].char_indices().next_back() {
                    start = i;
                    end = i + c.len_utf8();
                }
            }
            if end <= start {
                end = text[start..].chars().next().map_or(start, |c| start + c.len_utf8());
            }
            diagnostic(text, start..end, message, notes)
        }
        None => {
            // Ошибка в части движка: обычно функция пользователя не той формы.
            notes.push(format!("the engine calls `{}`", info.kind.signature()));
            notes.extend(labels.into_iter().map(|(_, l)| l).filter(|l| !l.is_empty() && *l != message));
            diagnostic(text, main_fn_range(text, info.kind), message, notes)
        }
    }
}

/// Возможности GPU при проверке: всё, что умеют настольные видеокарты, или минимум WebGL2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Desktop,
    WebGl2,
}

/// Разобрать и проверить собранный исходник naga. Ошибки — на строках пользователя.
fn validate(text: &str, info: &ShaderInfo, assembly: &Assembly, profile: Profile) -> Result<naga::Module, ShaderDiagnostic> {
    let module = match naga::front::wgsl::parse_str(&assembly.source) {
        Ok(module) => module,
        Err(error) => {
            let labels: Vec<(Range<usize>, String)> =
                error.labels().filter_map(|(span, label)| span.to_range().map(|r| (r, label.to_string()))).collect();
            let span = labels.first().map(|(r, _)| r.clone());
            return Err(map_error(text, info, assembly, span, error.message().to_string(), labels));
        }
    };
    let capabilities = match profile {
        Profile::Desktop => naga::valid::Capabilities::default(),
        Profile::WebGl2 => naga::valid::Capabilities::empty(),
    };
    let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities);
    if let Err(error) = validator.validate(&module) {
        let labels: Vec<(Range<usize>, String)> = error.spans().filter_map(|(span, label)| span.to_range().map(|r| (r, label.clone()))).collect();
        let mut message = error.as_inner().to_string();
        let mut source = std::error::Error::source(error.as_inner());
        while let Some(inner) = source {
            message.push_str(": ");
            message.push_str(&inner.to_string());
            source = inner.source();
        }
        // Место — самое «глубокое» (последнее) из мест в коде пользователя, иначе первое.
        let user = assembly.user_start..assembly.user_start + text.len();
        let span = labels.iter().rev().find(|(r, _)| user.contains(&r.start)).or(labels.first()).map(|(r, _)| r.clone());
        return Err(map_error(text, info, assembly, span, message, labels));
    }
    if profile == Profile::WebGl2 {
        check_webgl_limits(&module).map_err(|message| diagnostic(text, main_fn_range(text, info.kind), message, Vec::new()))?;
    }
    Ok(module)
}

/// Лимиты WebGL2 (`Limits::downlevel_webgl2_defaults`): текстуры, сэмплеры и uniform-блоки
/// на стадию, группы привязок, компоненты между стадиями.
fn check_webgl_limits(module: &naga::Module) -> Result<(), String> {
    let limits = wgpu::Limits::downlevel_webgl2_defaults();
    let (mut textures, mut samplers, mut uniforms, mut max_group) = (0u32, 0u32, 0u32, 0u32);
    for (_, var) in module.global_variables.iter() {
        let Some(binding) = &var.binding else { continue };
        max_group = max_group.max(binding.group);
        match &module.types[var.ty].inner {
            naga::TypeInner::Image { .. } => textures += 1,
            naga::TypeInner::Sampler { .. } => samplers += 1,
            _ if var.space == naga::AddressSpace::Uniform => uniforms += 1,
            _ => {}
        }
    }
    if textures > limits.max_sampled_textures_per_shader_stage {
        return Err(format!("too many textures for WebGL2: {textures} of {}", limits.max_sampled_textures_per_shader_stage));
    }
    if samplers > limits.max_samplers_per_shader_stage {
        return Err(format!("too many samplers for WebGL2: {samplers} of {}", limits.max_samplers_per_shader_stage));
    }
    if uniforms > limits.max_uniform_buffers_per_shader_stage {
        return Err(format!("too many uniform blocks for WebGL2: {uniforms} of {}", limits.max_uniform_buffers_per_shader_stage));
    }
    if max_group >= limits.max_bind_groups {
        return Err(format!("too many bind groups for WebGL2: {} of {}", max_group + 1, limits.max_bind_groups));
    }
    // Компоненты между стадиями: выход каждой вершинной точки входа.
    for entry in &module.entry_points {
        if entry.stage != naga::ShaderStage::Vertex {
            continue;
        }
        let Some(result) = &entry.function.result else { continue };
        let components = varying_components(module, result.ty, result.binding.as_ref());
        if components > limits.max_inter_stage_shader_components {
            return Err(format!(
                "`{}` passes {components} values between stages; WebGL2 allows {}",
                entry.name, limits.max_inter_stage_shader_components
            ));
        }
    }
    Ok(())
}

fn varying_components(module: &naga::Module, ty: naga::Handle<naga::Type>, binding: Option<&naga::Binding>) -> u32 {
    match &module.types[ty].inner {
        naga::TypeInner::Struct { members, .. } => {
            members.iter().map(|m| varying_components(module, m.ty, m.binding.as_ref())).sum()
        }
        inner => match binding {
            Some(naga::Binding::Location { .. }) => match inner {
                naga::TypeInner::Vector { size, .. } => *size as u32,
                _ => 1,
            },
            _ => 0,
        },
    }
}

/// Проверить ошибки заголовка и наличие главной функции до сборки.
fn precheck(text: &str) -> Result<ShaderInfo, ShaderDiagnostic> {
    let info = parse_header(text).map_err(|e| {
        let line_start: usize = text.split_inclusive('\n').take(e.line - 1).map(str::len).sum();
        let line_text = text[line_start..].lines().next().unwrap_or("");
        let start = line_start + line_text.char_indices().nth(e.column.saturating_sub(1)).map_or(line_text.len(), |(i, _)| i);
        let end = if e.len == 0 {
            line_start + line_text.len()
        } else {
            let rest = &text[start..];
            start + rest.char_indices().nth(e.len).map_or(rest.lines().next().map_or(0, str::len), |(i, _)| i)
        };
        ShaderDiagnostic { line: e.line, column: e.column, range: start..end.max(start), message: e.message, notes: Vec::new() }
    })?;
    let textures = info.textures().count();
    if textures > MAX_TEXTURE_PARAMS {
        let extra: &ParamDecl = info.textures().nth(MAX_TEXTURE_PARAMS).expect("counted");
        let line_start: usize = text.split_inclusive('\n').take(extra.line - 1).map(str::len).sum();
        let len = text[line_start..].lines().next().map_or(0, str::len);
        return Err(diagnostic(
            text,
            line_start..line_start + len,
            format!("at most {MAX_TEXTURE_PARAMS} texture parameters fit every platform (WebGL2 has 16 texture units, the engine uses up to 10)"),
            Vec::new(),
        ));
    }
    // Текстура-параметр — глобальная переменная: её имя не должно совпасть со встроенным.
    if let Some(clash) = info.textures().find(|p| BUILTIN_GLOBALS.contains(&p.name.as_str())) {
        let line_start: usize = text.split_inclusive('\n').take(clash.line - 1).map(str::len).sum();
        let len = text[line_start..].lines().next().map_or(0, str::len);
        return Err(diagnostic(
            text,
            line_start..line_start + len,
            format!("`{}` is a built-in name; give the texture another name", clash.name),
            Vec::new(),
        ));
    }
    let code = strip_comments(text);
    if find_fn(&code, info.kind.function()).is_none() {
        return Err(diagnostic(
            text,
            0..text.lines().next().map_or(0, str::len),
            format!("a {} shader needs the function `{}`", info.kind.keyword(), info.kind.signature()),
            Vec::new(),
        ));
    }
    Ok(info)
}

/// Проверить текст `.shader` так, как его соберёт движок (вариант 0 для настольных GPU и
/// для лимитов WebGL2). Пустой список — шейдер верен. Для вкладки Code редактора.
pub fn check(text: &str) -> Vec<ShaderDiagnostic> {
    match compile_variants(text, &[0]) {
        Ok(compiled) => {
            let assembly = assemble(text, &compiled.info, 0);
            match validate(text, &compiled.info, &assembly, Profile::WebGl2) {
                Ok(_) => Vec::new(),
                Err(d) => vec![d],
            }
        }
        Err(d) => vec![d],
    }
}

/// Проверить шейдер и собрать исходники всех его вариантов (для рендерера). Проверяются
/// все варианты: до GPU доходит только то, что naga признала верным.
pub fn compile(text: &str) -> Result<Compiled, ShaderDiagnostic> {
    compile_variants(text, &(0..=SHADOW_PASS).collect::<Vec<_>>())
}

/// Проверить шейдер во всех вариантах и с лимитами WebGL2 (для тестов примеров).
pub fn check_everywhere(text: &str) -> Result<Compiled, ShaderDiagnostic> {
    let compiled = compile(text)?;
    for variant in variants_of(compiled.info.kind) {
        let assembly = assemble(text, &compiled.info, variant);
        validate(text, &compiled.info, &assembly, Profile::WebGl2)?;
    }
    Ok(compiled)
}

fn variants_of(kind: ShaderKind) -> Vec<usize> {
    if kind.is_material() {
        (0..=SHADOW_PASS).collect()
    } else {
        vec![0]
    }
}

/// Проверить naga варианты `validate_variants` (остальные только собрать).
fn compile_variants(text: &str, validate_variants: &[usize]) -> Result<Compiled, ShaderDiagnostic> {
    let info = precheck(text)?;
    let code = strip_comments(text);
    let mut sources = Vec::new();
    for variant in variants_of(info.kind) {
        let assembly = assemble(text, &info, variant);
        if validate_variants.contains(&variant) {
            validate(text, &info, &assembly, Profile::Desktop)?;
        }
        sources.push(assembly.source);
    }
    Ok(Compiled {
        layout: ParamLayout::of(&info),
        has_vertex: info.kind.is_material() && find_fn(&code, "vertex").is_some(),
        has_discard: has_word(&code, "discard"),
        info,
        sources,
    })
}

/// Шаблон нового шейдера вида `kind` (Create → Shader): работает сразу, комментарии
/// по-русски — как у шаблонов скриптов.
pub fn template(kind: ShaderKind) -> &'static str {
    match kind {
        ShaderKind::Surface => include_str!("template_surface.shader"),
        ShaderKind::Unlit => include_str!("template_unlit.shader"),
        ShaderKind::Sprite => include_str!("template_sprite.shader"),
        ShaderKind::Post => include_str!("template_post.shader"),
    }
}

#[cfg(test)]
mod tests;

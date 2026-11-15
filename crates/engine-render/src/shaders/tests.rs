//! Сборка и проверка своих шейдеров без GPU: шаблоны, раскладка параметров, ошибки на
//! строках пользователя.

use super::*;

const DISSOLVE: &str = "//! kind: surface
//! param edge_color: color = (1.0, 0.5, 0.1, 1.0)
//! param amount: float = 0.0 [0, 1]
//! param noise: texture = white
fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);
    let n = textureSample(noise, noise_sampler, in.uv).r;
    if (n < params.amount) {
        discard;
    }
    s.emissive += params.edge_color.rgb * step(n, params.amount + 0.05);
    return s;
}
";

#[test]
fn templates_compile_in_every_variant_and_for_webgl2() {
    for kind in ShaderKind::ALL {
        let text = template(kind);
        if let Err(d) = check_everywhere(text) {
            panic!("{kind:?} template: {d}");
        }
        assert!(check(text).is_empty(), "{kind:?}");
    }
}

#[test]
fn the_brief_example_compiles_with_discard_and_textures() {
    let compiled = check_everywhere(DISSOLVE).unwrap_or_else(|d| panic!("{d}"));
    assert!(compiled.has_discard && !compiled.has_vertex);
    assert_eq!(compiled.sources.len(), VARIANT_COUNT + 1);
    assert!(!compiled.sources[SHADOW_PASS].contains("shadow_map"), "the shadow pass does not read the shadow map");
    assert_eq!(compiled.layout.textures, vec![2]);
}

/// Смещения uniform-блока совпадают с тем, как WGSL раскладывает `EngineParamsRaw`.
#[test]
fn param_layout_matches_the_wgsl_layout() {
    let text = "//! kind: post
//! param a: float = 1
//! param b: vec3 = (1, 2, 3)
//! param c: vec2 = (4, 5)
//! param d: bool = true
//! param e: color = (0.1, 0.2, 0.3, 0.4)
//! param f: int = -7
//! param t: texture
fn post(in: PostIn) -> vec4<f32> { return in.color * params.a; }
";
    let compiled = compile(text).unwrap_or_else(|d| panic!("{d}"));
    let module = naga::front::wgsl::parse_str(&compiled.sources[0]).unwrap();
    let mut layouter = naga::proc::Layouter::default();
    layouter.update(module.to_ctx()).unwrap();
    let (handle, ty) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("EngineParamsRaw")).unwrap();
    let naga::TypeInner::Struct { members, .. } = &ty.inner else { panic!() };
    let offsets: Vec<(String, u32)> = members.iter().map(|m| (m.name.clone().unwrap(), m.offset)).collect();
    let mine: Vec<(String, u32)> =
        compiled.layout.fields.iter().map(|&(i, o)| (compiled.info.params[i].name.clone(), o)).collect();
    assert_eq!(offsets, mine);
    assert!(compiled.layout.size >= layouter[handle].size && compiled.layout.size.is_multiple_of(16));
    // Значения: bool — u32, int — i32, цвет — четыре числа.
    let bytes = compiled.layout.bytes(&compiled.info, &[&ShaderParams::new().with("f", 3)]);
    let at = |name: &str| compiled.layout.fields.iter().find(|(i, _)| compiled.info.params[*i].name == name).unwrap().1 as usize;
    assert_eq!(&bytes[at("d")..at("d") + 4], &1u32.to_le_bytes());
    assert_eq!(&bytes[at("f")..at("f") + 4], &3i32.to_le_bytes());
    assert_eq!(&bytes[at("e") + 12..at("e") + 16], &0.4f32.to_le_bytes());
}

fn first_error(text: &str) -> ShaderDiagnostic {
    let found = check(text);
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn syntax_errors_point_at_the_users_line() {
    let text = "//! kind: sprite
//! param amount: float = 0.5

fn sprite(in: SpriteIn) -> vec4<f32> {
    let x = ;
    return default_sprite(in);
}
";
    let d = first_error(text);
    assert_eq!(d.line, 5, "{d}");
    assert_eq!(&text[d.range.clone()], ";", "{d}");
}

#[test]
fn unknown_fields_and_params_suggest_the_right_name() {
    let text = "//! kind: unlit
//! param amount: float = 0.5
fn unlit(in: SurfaceIn) -> vec4<f32> {
    return vec4<f32>(in.uvv, 0.0, params.amount);
}
";
    let d = first_error(text);
    assert_eq!(d.line, 4, "{d}");
    assert!(d.notes.iter().any(|n| n.contains("did you mean `uv`")), "{d}");

    let text = "//! kind: unlit
//! param amount: float = 0.5
fn unlit(in: SurfaceIn) -> vec4<f32> {
    let a = params.amout;
    return vec4<f32>(a);
}
";
    let d = first_error(text);
    assert_eq!(d.line, 4, "{d}");
    assert!(d.notes.iter().any(|n| n.contains("did you mean `amount`")), "{d}");

    let text = "//! kind: post
fn post(in: PostIn) -> vec4<f32> {
    return vec4<f32>(colr, 1.0);
}
";
    let d = first_error(text);
    assert_eq!(d.line, 3, "{d}");
}

#[test]
fn type_errors_in_the_users_code_are_on_its_line() {
    let text = "//! kind: post
fn post(in: PostIn) -> vec4<f32> {
    let a: f32 = in.uv;
    return in.color;
}
";
    let d = first_error(text);
    assert_eq!(d.line, 3, "{d}");
}

#[test]
fn a_wrong_signature_points_at_the_function() {
    let text = "//! kind: surface

fn surface(in: SurfaceIn) -> vec4<f32> {
    return vec4<f32>(1.0);
}
";
    let d = first_error(text);
    assert_eq!(d.line, 3, "{d}");
    assert!(d.notes.iter().any(|n| n.contains("fn surface(in: SurfaceIn) -> Surface")), "{d}");
    let d = first_error("//! kind: sprite\nfn sprit(in: SpriteIn) -> vec4<f32> { return in.color; }\n");
    assert!(d.message.contains("fn sprite(in: SpriteIn)"), "{d}");
}

#[test]
fn header_errors_are_located() {
    let text = "//! kind: post\n//! param amount: flaot = 0.5\nfn post(in: PostIn) -> vec4<f32> { return in.color; }\n";
    let d = first_error(text);
    assert_eq!((d.line, d.column), (2, 19));
    assert_eq!(&text[d.range.clone()], "flaot");
    let text = "//! kind: post
//! param a: texture
//! param b: texture
//! param c: texture
//! param d: texture
//! param e: texture
fn post(in: PostIn) -> vec4<f32> { return in.color; }
";
    let d = first_error(text);
    assert_eq!(d.line, 6, "{d}");
}

#[test]
fn a_texture_named_like_a_builtin_is_explained() {
    let d = first_error("//! kind: post\n//! param fbm: texture\nfn post(in: PostIn) -> vec4<f32> { return in.color; }\n");
    assert_eq!(d.line, 2, "{d}");
    assert!(d.message.contains("built-in"), "{d}");
}

#[test]
fn vertex_displacement_and_object_space_compile() {
    let text = "//! kind: surface
//! param sway: float = 0.3 [0, 1]
fn vertex(in: VertexIn) -> VertexOut {
    var o = default_vertex(in);
    o.world_position.x += sin(in.time + in.world_position.z) * params.sway * max(in.position.y, 0.0);
    return o;
}
fn surface(in: SurfaceIn) -> Surface {
    var s = default_surface(in);
    s.albedo = s.albedo * (0.5 + 0.5 * fract(in.object_position.y));
    return s;
}
";
    let compiled = check_everywhere(text).unwrap_or_else(|d| panic!("{d}"));
    assert!(compiled.has_vertex);
    assert!(compiled.sources[0].contains("const ENGINE_OBJECT_SPACE: bool = true;"));
    assert!(!compiled.sources[0].contains("return default_vertex(in);\n}\n// ----"), "no second `vertex`");
}

#[test]
fn comments_do_not_confuse_the_scanner() {
    let text = "//! kind: post\n// fn post is below; object_position, discard\n/* fn vertex */\nfn post(in: PostIn) -> vec4<f32> { return in.color; }\n";
    let compiled = compile(text).unwrap_or_else(|d| panic!("{d}"));
    assert!(!compiled.has_discard && !compiled.has_vertex);
}

/// Примеры из `assets/shaders` собираются во всех вариантах и укладываются в лимиты WebGL2.
#[test]
fn every_example_shader_compiles_everywhere() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shaders");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("assets/shaders exists") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "shader") {
            let text = std::fs::read_to_string(&path).unwrap();
            if let Err(d) = check_everywhere(&text) {
                panic!("{}: {d}", path.display());
            }
            count += 1;
        }
    }
    assert!(count >= 15, "{count} examples");
}

/// Пропущенная `;` — ошибка на строке, где её нет, а не на следующей.
#[test]
fn a_missing_semicolon_is_reported_on_its_line() {
    let text = "//! kind: unlit\nfn unlit(in: SurfaceIn) -> vec4<f32> {\n    return vec4<f32>(1.0)\n}\n";
    let d = first_error(text);
    assert_eq!(d.line, 3, "{d}");
}
